//! `-R`: the server listens and opens a `forwarded-tcpip` channel per connection, which
//! [`crate::ssh`]'s client handler hands to the owning rule through [`RemoteRoutes`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use russh::client::{ChannelOpenHandle, Msg};
use russh::{Channel, ChannelOpenFailure};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use super::{bridge, host_port, Ctx, ForwardRule, Tracker, CONNECT_TIMEOUT};
use crate::error::Error;

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
        let routes = self.0.lock().unwrap();
        // Servers normally echo the requested address, but match on the port alone if a
        // server spells it differently.
        let rule = routes
            .get(&(address.to_owned(), port))
            .or_else(|| routes.iter().find(|((_, p), _)| *p == port).map(|(_, rule)| rule));
        if let Some(rule) = rule {
            let _ = rule.send(incoming);
        }
    }
}

/// Unregisters the route and cancels the server-side listener when the rule's task ends.
/// The cancellation is recorded so that restarting the rule waits for it.
struct Registration {
    ctx: Ctx,
    address: String,
    port: u32,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let shared = &self.ctx.shared;
        shared.routes.0.lock().unwrap().remove(&(self.address.clone(), self.port));
        let (handle, address, port) = (self.ctx.handle(), self.address.clone(), self.port);
        let cancel = tauri::async_runtime::spawn(async move {
            // Fails harmlessly when the connection is already closing.
            let _ = handle.cancel_tcpip_forward(address, port).await;
        });
        shared.cancels.lock().unwrap().insert(self.ctx.rule_id.clone(), cancel);
    }
}

pub async fn run(rule: &ForwardRule, ctx: &Ctx) -> Result<()> {
    let handle = ctx.handle();
    let requested = host_port(&rule.bind_host, rule.bind_port);
    let allocated = handle
        .tcpip_forward(rule.bind_host.clone(), rule.bind_port.into())
        .await
        .context(Error::new("forward.remoteRefused").param("address", &requested))?;
    // The server only reports the port when it picked one.
    let port = if rule.bind_port == 0 { allocated } else { rule.bind_port.into() };

    let (sender, mut incoming) = mpsc::unbounded_channel();
    let address = rule.bind_host.clone();
    ctx.shared.routes.0.lock().unwrap().insert((address.clone(), port), sender);
    let _registration = Registration { ctx: ctx.clone(), address, port };

    let mut tracker = Tracker::new(ctx, host_port(&rule.bind_host, port));
    loop {
        tokio::select! {
            Some(next) = incoming.recv() => {
                let (host, port) = (rule.target_host.clone(), rule.target_port);
                tracker.spawn(async move { connect(next, host, port).await });
            }
            finished = tracker.next_finished() => tracker.finished(finished),
        }
    }
}

/// Connects to the target first and only then accepts the channel, so an unreachable
/// target is reported to the server as a failed open, as OpenSSH does.
async fn connect(incoming: Incoming, host: String, port: u16) -> Result<()> {
    let target = host_port(&host, port);
    let connected = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port))).await {
        Ok(result) => result.context(Error::new("forward.connectFailed").param("target", &target)),
        Err(_) => Err(Error::new("forward.connectTimeout").param("target", &target).into()),
    };
    match connected {
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
