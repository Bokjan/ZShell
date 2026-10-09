//! `-L`: listen on this computer; each connection is carried to the target through a
//! `direct-tcpip` channel opened from the server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};

use super::{bridge, host_port, Ctx, ForwardRule, Tracker};
use crate::error::Error;
use crate::ssh::SshHandle;

pub async fn run(rule: &ForwardRule, ctx: &Ctx) -> Result<()> {
    let listener = Listener::bind(&rule.bind_host, rule.bind_port).await?;
    let mut tracker = Tracker::new(ctx, listener.address(&rule.bind_host)?);
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let (handle, host, port) = (ctx.handle(), rule.target_host.clone(), rule.target_port);
                    tracker.spawn(async move { connect(handle, stream, peer, host, port).await });
                }
                Err(e) => accept_failed(&mut tracker, e).await,
            },
            finished = tracker.next_finished() => tracker.finished(finished),
        }
    }
}

/// Where a local or dynamic rule listens: every address its bind host resolves to, as
/// OpenSSH does, so a rule on `localhost` takes connections to both 127.0.0.1 and ::1
/// (clients differ in which one they try).
pub(super) struct Listener(Vec<TcpListener>);

impl Listener {
    /// Fails only if no address could be listened on.
    pub(super) async fn bind(host: &str, port: u16) -> Result<Self> {
        let failed = || Error::new("forward.bindFailed").param("address", host_port(host, port));
        let mut addresses: Vec<SocketAddr> = Vec::new();
        for address in tokio::net::lookup_host((host, port)).await.with_context(failed)? {
            if !addresses.contains(&address) {
                addresses.push(address);
            }
        }
        let mut listeners: Vec<TcpListener> = Vec::new();
        let mut error = None;
        for mut address in addresses {
            // With port 0, the others take the port picked for the first.
            if let Some(first) = listeners.first() {
                address.set_port(first.local_addr()?.port());
            }
            match TcpListener::bind(address).await {
                Ok(listener) => listeners.push(listener),
                Err(e) => {
                    error.get_or_insert(e);
                }
            }
        }
        match error {
            Some(e) if listeners.is_empty() => Err(anyhow::Error::from(e).context(failed())),
            _ if listeners.is_empty() => Err(failed().into()),
            _ => Ok(Self(listeners)),
        }
    }

    /// The address to show: the one listened on (with the real port when 0 was asked for),
    /// or `host` with that port when there are several.
    pub(super) fn address(&self, host: &str) -> std::io::Result<String> {
        let first = self.0[0].local_addr()?;
        Ok(if self.0.len() > 1 { host_port(host, first.port()) } else { first.to_string() })
    }

    pub(super) async fn accept(&self) -> std::io::Result<(TcpStream, SocketAddr)> {
        std::future::poll_fn(|cx| {
            for listener in &self.0 {
                if let Poll::Ready(accepted) = listener.poll_accept(cx) {
                    return Poll::Ready(accepted);
                }
            }
            Poll::Pending
        })
        .await
    }
}

/// Accept errors are usually transient (aborted handshakes, descriptor exhaustion); report
/// and keep listening, pausing briefly so a persistent error cannot spin.
pub(super) async fn accept_failed(tracker: &mut Tracker<'_>, error: std::io::Error) {
    tracker.failed(Error::new("forward.acceptFailed").detail(error));
    tokio::time::sleep(Duration::from_millis(100)).await;
}

async fn connect(handle: Arc<SshHandle>, stream: TcpStream, peer: SocketAddr, host: String, port: u16) -> Result<()> {
    let target = host_port(&host, port);
    let channel = handle
        .channel_open_direct_tcpip(host, port.into(), peer.ip().to_string(), peer.port().into())
        .await
        .context(Error::new("forward.channelFailed").param("target", target))?;
    bridge(stream, channel).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listens_on_every_address_of_localhost() {
        let listener = Listener::bind("localhost", 0).await.unwrap();
        let port = listener.0[0].local_addr().unwrap().port();
        let mut expected: Vec<SocketAddr> = Vec::new();
        for address in tokio::net::lookup_host(("localhost", port)).await.unwrap() {
            if !expected.contains(&address) {
                expected.push(address);
            }
        }
        let bound: Vec<SocketAddr> = listener.0.iter().map(|l| l.local_addr().unwrap()).collect();
        assert_eq!(bound, expected);
        for address in expected {
            let _client = TcpStream::connect(address).await.unwrap();
            let (_, peer) = listener.accept().await.unwrap();
            assert_eq!(peer.ip(), address.ip());
        }
    }
}
