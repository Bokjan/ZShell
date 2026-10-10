//! `-D`: a SOCKS proxy on this computer; each CONNECT request is carried through a
//! `direct-tcpip` channel, so host names are resolved by the server.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use russh::ChannelOpenFailure;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::local::{accept_failed, Listener};
use super::socks::{self, Reply};
use super::{bridge, host_port, Ctx, ForwardRule, Stopping, Tracker};
use crate::error::Error;
use crate::ssh::SshHandle;

/// How long a client may take to send its request after connecting.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(rule: &ForwardRule, ctx: &Ctx, stopping: &mut Stopping) -> Result<()> {
    let listener = Listener::bind(&rule.bind_host, rule.bind_port).await?;
    let mut tracker = Tracker::new(ctx, listener.address(&rule.bind_host)?);
    loop {
        let report = tracker.report_due();
        tokio::select! {
            () = super::at(report) => tracker.report(),
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let handle = ctx.handle();
                    tracker.spawn(async move { connect(handle, stream, peer).await });
                }
                Err(e) => accept_failed(&mut tracker, e).await,
            },
            finished = tracker.next_finished() => tracker.finished(finished),
            () = stopping.requested() => return Ok(()),
        }
    }
}

async fn connect(handle: Arc<SshHandle>, mut stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let Some(request) = request(&mut stream, REQUEST_TIMEOUT).await? else {
        return Ok(());
    };
    let target = host_port(&request.host, request.port);
    let opened = handle
        .channel_open_direct_tcpip(request.host, request.port.into(), peer.ip().to_string(), peer.port().into())
        .await;
    let channel = match opened {
        Ok(channel) => channel,
        Err(e) => {
            let reply = match e {
                russh::Error::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed) => Reply::ConnectionRefused,
                russh::Error::ChannelOpenFailure(ChannelOpenFailure::AdministrativelyProhibited) => Reply::NotAllowed,
                _ => Reply::GeneralFailure,
            };
            let _ = socks::reply(&mut stream, request.version, reply).await;
            return Err(e).context(Error::new("forward.channelFailed").param("target", target));
        }
    };
    socks::reply(&mut stream, request.version, Reply::Succeeded).await?;
    bridge(stream, channel).await;
    Ok(())
}

/// The client's request; `None` if none came in time: a port scanner or a stuck client that
/// connected and sent nothing would otherwise hold its connection (and a task) for good. Not
/// an error of the rule's.
async fn request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, timeout: Duration) -> Result<Option<socks::Request>> {
    match tokio::time::timeout(timeout, socks::accept(stream)).await {
        Ok(request) => request.map(Some),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn gives_up_on_clients_that_send_nothing() {
        let (mut ours, _client) = tokio::io::duplex(64);
        assert!(request(&mut ours, REQUEST_TIMEOUT).await.unwrap().is_none());

        let (mut ours, mut client) = tokio::io::duplex(64);
        tokio::io::AsyncWriteExt::write_all(&mut client, &[4, 1, 0, 80, 10, 0, 0, 1, 0]).await.unwrap();
        let request = request(&mut ours, REQUEST_TIMEOUT).await.unwrap().unwrap();
        assert_eq!((request.host.as_str(), request.port), ("10.0.0.1", 80));
    }
}
