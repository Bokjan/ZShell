//! `-L`: listen on this computer; each connection is carried to the target through a
//! `direct-tcpip` channel opened from the server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};

use super::{bridge, host_port, Ctx, ForwardRule, Tracker};
use crate::error::Error;
use crate::ssh::SshHandle;

pub async fn run(rule: &ForwardRule, ctx: &Ctx) -> Result<()> {
    let listener = listen(&rule.bind_host, rule.bind_port).await?;
    let mut tracker = Tracker::new(ctx, listener.local_addr()?.to_string());
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

pub(super) async fn listen(host: &str, port: u16) -> Result<TcpListener> {
    TcpListener::bind((host, port))
        .await
        .context(Error::new("forward.bindFailed").param("address", host_port(host, port)))
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
