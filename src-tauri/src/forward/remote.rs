//! `-R`: the server listens and opens a `forwarded-tcpip` channel per connection, which
//! [`crate::ssh`]'s client handler hands to the owning rule through the connection's
//! [`RemoteRoutes`].

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use russh::ChannelOpenFailure;
use tokio::sync::mpsc;

use super::{bridge, host_port, Ctx, ForwardRule, InFlight, RuleTraffic, Stopping, Tracker};
use crate::error::Error;
use crate::model::ALL_ADDRESSES;
use crate::ssh::{Incoming, Liveness, RemoteRoutes, SshHandle, ROUTE_QUEUE};

/// How long a stopping rule waits for the server to cancel its listener.
const CANCEL_TIMEOUT: Duration = Duration::from_secs(10);

/// The server's listener for a rule, with the rule's route to it. Cancelled by
/// [`Listening::cancel`] when the rule stops; dropped otherwise (the task aborted), it still
/// removes the route and cancels the listener in the background.
struct Listening {
    handle: Arc<SshHandle>,
    routes: RemoteRoutes,
    address: String,
    port: u32,
    cancelled: bool,
}

impl Listening {
    fn new(ctx: &Ctx, address: String, port: u32, route: mpsc::Sender<Incoming>) -> Self {
        let routes = ctx.shared.routes.clone();
        routes.insert(address.clone(), port, route);
        Self { handle: ctx.handle(), routes, address, port, cancelled: false }
    }

    /// Removes the route and waits for the server to cancel the listener, so that the port
    /// is free on the server once this returns. Fails harmlessly when the connection is
    /// closing.
    async fn cancel(mut self) {
        self.cancelled = true;
        self.routes.remove(&self.address, self.port);
        let cancel = self.handle.cancel_tcpip_forward(self.address.clone(), self.port);
        let _ = tokio::time::timeout(CANCEL_TIMEOUT, cancel).await;
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        if self.cancelled {
            return;
        }
        self.routes.remove(&self.address, self.port);
        let (handle, address, port) = (self.handle.clone(), self.address.clone(), self.port);
        tauri::async_runtime::spawn(async move {
            let _ = handle.cancel_tcpip_forward(address, port).await;
        });
    }
}

pub async fn run(rule: &ForwardRule, ctx: &Ctx, stopping: &mut Stopping) -> Result<InFlight> {
    let handle = ctx.handle();
    let requested = host_port(&rule.bind_host, rule.bind_port);
    // Every address is the empty one in the protocol (RFC 4254 7.1), as OpenSSH sends `*`.
    let address = if rule.bind_host == ALL_ADDRESSES { String::new() } else { rule.bind_host.clone() };
    let request = handle.tcpip_forward(address.clone(), rule.bind_port.into());
    tokio::pin!(request);
    // The server only reports the port when it picked one.
    let port = |allocated: u32| if rule.bind_port == 0 { allocated } else { rule.bind_port.into() };
    let reply = tokio::select! {
        reply = &mut request => reply,
        () = stopping.requested() => {
            // russh doesn't withdraw a request when its caller goes away, and the server may
            // be listening already: wait for the reply and cancel the listener, so that it
            // doesn't stay without a route (refusing connections, and restarts of the rule
            // with "address in use").
            if let Ok(Ok(allocated)) = tokio::time::timeout(CANCEL_TIMEOUT, request).await {
                let cancel = handle.cancel_tcpip_forward(address.clone(), port(allocated));
                let _ = tokio::time::timeout(CANCEL_TIMEOUT, cancel).await;
            }
            return Ok(InFlight::default());
        }
    };
    let port = port(reply.context(Error::new("forward.remoteRefused").param("address", &requested))?);

    let (sender, mut incoming) = mpsc::channel(ROUTE_QUEUE);
    let listening = Listening::new(ctx, address, port, sender);
    let mut tracker = Tracker::new(ctx, host_port(&rule.bind_host, port));
    loop {
        let report = tracker.report_due();
        tokio::select! {
            () = super::at(report) => tracker.report(),
            Some(next) = incoming.recv() => {
                let (host, port, liveness, traffic) = (rule.target_host.clone(), rule.target_port, ctx.liveness(), ctx.traffic.clone());
                tracker.spawn(async move { connect(next, host, port, liveness, traffic).await });
            }
            finished = tracker.next_finished() => tracker.finished(finished),
            () = stopping.requested() => break,
        }
    }
    // Those not given back end before the listener is cancelled.
    let in_flight = tracker.into_in_flight(stopping);
    listening.cancel().await;
    Ok(in_flight)
}

/// Connects to the target first and only then accepts the channel, so an unreachable
/// target is reported to the server as a failed open, as OpenSSH does. Each of the target's
/// addresses is tried in turn, as for a session's host.
async fn connect(incoming: Incoming, host: String, port: u16, liveness: Liveness, traffic: Arc<RuleTraffic>) -> Result<()> {
    match crate::net::tcp(&host, port).await {
        Ok(stream) => {
            incoming.reply.accept().await;
            bridge(stream, incoming.channel, liveness, traffic).await;
            Ok(())
        }
        Err(e) => {
            incoming.reply.reject(ChannelOpenFailure::ConnectFailed).await;
            Err(e)
        }
    }
}
