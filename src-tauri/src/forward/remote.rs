//! `-R`: the server listens and opens a `forwarded-tcpip` channel per connection, which
//! [`crate::ssh`]'s client handler hands to the owning rule through [`RemoteRoutes`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use russh::client::{ChannelOpenHandle, Msg};
use russh::{Channel, ChannelOpenFailure};
use tokio::sync::mpsc;

use super::{bridge, host_port, Ctx, ForwardRule, Stopping, Tracker};
use crate::error::Error;
use crate::ssh::SshHandle;

/// A connection the server accepted on a remote forward, not yet confirmed.
pub struct Incoming {
    pub channel: Channel<Msg>,
    /// Dropping it rejects the channel.
    pub reply: ChannelOpenHandle,
}

/// Remote forwards active on a connection, keyed by the address and port the server
/// listens on.
#[derive(Clone, Default)]
pub struct RemoteRoutes(Arc<Mutex<HashMap<(String, u32), Route>>>);

type Route = mpsc::UnboundedSender<Incoming>;

impl RemoteRoutes {
    /// Delivers a `forwarded-tcpip` channel to its rule; unknown ones are rejected.
    pub fn route(&self, address: &str, port: u32, incoming: Incoming) {
        if let Some(rule) = find_route(&self.0.lock().unwrap(), address, port) {
            let _ = rule.send(incoming);
        }
    }
}

/// The route for a channel. Servers normally echo the requested address (OpenSSH does), but
/// one that spells it differently (`localhost` for `127.0.0.1`, say) is matched on the port
/// alone, as long as only one rule listens on it: with two, the connection may be meant for
/// either, and is rejected rather than sent to the wrong one.
fn find_route<'a, R>(routes: &'a HashMap<(String, u32), R>, address: &str, port: u32) -> Option<&'a R> {
    if let Some(route) = routes.get(&(address.to_owned(), port)) {
        return Some(route);
    }
    let mut on_port = routes.iter().filter(|((_, p), _)| *p == port).map(|(_, route)| route);
    match (on_port.next(), on_port.next()) {
        (Some(route), None) => Some(route),
        _ => None,
    }
}

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
    fn new(ctx: &Ctx, address: String, port: u32, route: Route) -> Self {
        let routes = ctx.shared.routes.clone();
        routes.0.lock().unwrap().insert((address.clone(), port), route);
        Self { handle: ctx.handle(), routes, address, port, cancelled: false }
    }

    /// Removes the route and waits for the server to cancel the listener, so that the port
    /// is free on the server once this returns. Fails harmlessly when the connection is
    /// closing.
    async fn cancel(mut self) {
        self.cancelled = true;
        self.routes.0.lock().unwrap().remove(&(self.address.clone(), self.port));
        let cancel = self.handle.cancel_tcpip_forward(self.address.clone(), self.port);
        let _ = tokio::time::timeout(CANCEL_TIMEOUT, cancel).await;
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        if self.cancelled {
            return;
        }
        self.routes.0.lock().unwrap().remove(&(self.address.clone(), self.port));
        let (handle, address, port) = (self.handle.clone(), self.address.clone(), self.port);
        tauri::async_runtime::spawn(async move {
            let _ = handle.cancel_tcpip_forward(address, port).await;
        });
    }
}

pub async fn run(rule: &ForwardRule, ctx: &Ctx, stopping: &mut Stopping) -> Result<()> {
    let handle = ctx.handle();
    let requested = host_port(&rule.bind_host, rule.bind_port);
    let request = handle.tcpip_forward(rule.bind_host.clone(), rule.bind_port.into());
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
                let cancel = handle.cancel_tcpip_forward(rule.bind_host.clone(), port(allocated));
                let _ = tokio::time::timeout(CANCEL_TIMEOUT, cancel).await;
            }
            return Ok(());
        }
    };
    let port = port(reply.context(Error::new("forward.remoteRefused").param("address", &requested))?);

    let (sender, mut incoming) = mpsc::unbounded_channel();
    let listening = Listening::new(ctx, rule.bind_host.clone(), port, sender);
    let mut tracker = Tracker::new(ctx, host_port(&rule.bind_host, port));
    loop {
        let report = tracker.report_due();
        tokio::select! {
            () = super::at(report) => tracker.report(),
            Some(next) = incoming.recv() => {
                let (host, port) = (rule.target_host.clone(), rule.target_port);
                tracker.spawn(async move { connect(next, host, port).await });
            }
            finished = tracker.next_finished() => tracker.finished(finished),
            () = stopping.requested() => break,
        }
    }
    drop(tracker);
    listening.cancel().await;
    Ok(())
}

/// Connects to the target first and only then accepts the channel, so an unreachable
/// target is reported to the server as a failed open, as OpenSSH does. Each of the target's
/// addresses is tried in turn, as for a session's host.
async fn connect(incoming: Incoming, host: String, port: u16) -> Result<()> {
    match crate::net::tcp(&host, port).await {
        Ok(stream) => {
            incoming.reply.accept().await;
            bridge(stream, incoming.channel).await;
            Ok(())
        }
        Err(e) => {
            incoming.reply.reject(ChannelOpenFailure::ConnectFailed).await;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_by_address_then_by_port_alone_when_unambiguous() {
        let mut routes = HashMap::new();
        routes.insert(("127.0.0.1".to_owned(), 9000), "loopback");
        assert_eq!(find_route(&routes, "127.0.0.1", 9000), Some(&"loopback"));
        assert_eq!(find_route(&routes, "localhost", 9000), Some(&"loopback"));
        assert_eq!(find_route(&routes, "127.0.0.1", 9001), None);

        routes.insert(("10.0.0.1".to_owned(), 9000), "lan");
        assert_eq!(find_route(&routes, "10.0.0.1", 9000), Some(&"lan"));
        assert_eq!(find_route(&routes, "localhost", 9000), None);
    }
}
