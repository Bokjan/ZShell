//! `-D`: a SOCKS proxy on this computer; each CONNECT request is carried through a
//! `direct-tcpip` channel, so host names are resolved by the server.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use russh::ChannelOpenFailure;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use super::local::serve;
use super::socks::{self, Reply};
use super::{bridge, host_port, Ctx, ForwardRule, Opener, Stopping};
use crate::error::Error;

/// How long a client may take to send its request after connecting.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(rule: &ForwardRule, ctx: &Ctx, stopping: &mut Stopping) -> Result<()> {
    serve(rule, ctx, stopping, |stream, peer| connect(ctx.opener(), stream, peer)).await
}

async fn connect(opener: Opener, mut stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let Some(request) = request(&mut stream, REQUEST_TIMEOUT).await? else {
        return Ok(());
    };
    let target = host_port(&request.host, request.port);
    let channel = match opener.open(request.host, request.port, peer).await {
        Ok(channel) => channel,
        Err(e) => {
            let reply = match e.downcast_ref::<russh::Error>() {
                Some(russh::Error::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed)) => Reply::ConnectionRefused,
                Some(russh::Error::ChannelOpenFailure(ChannelOpenFailure::AdministrativelyProhibited)) => Reply::NotAllowed,
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

/// The client's request; `None` if it closed the connection first, or if none came in time:
/// a port scanner or a stuck client that connected and sent nothing would otherwise hold its
/// connection (and a task) for good. Neither is an error of the rule's.
async fn request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, timeout: Duration) -> Result<Option<socks::Request>> {
    tokio::time::timeout(timeout, socks::accept(stream)).await.unwrap_or(Ok(None))
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

        // A browser's connection made ahead and closed unused.
        let (mut ours, client) = tokio::io::duplex(64);
        drop(client);
        assert!(super::request(&mut ours, REQUEST_TIMEOUT).await.unwrap().is_none());
    }
}
