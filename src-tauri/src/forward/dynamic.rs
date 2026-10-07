//! `-D`: a SOCKS proxy on this computer; each CONNECT request is carried through a
//! `direct-tcpip` channel, so host names are resolved by the server.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use russh::ChannelOpenFailure;
use tokio::net::TcpStream;

use super::local::{accept_failed, listen};
use super::socks::{self, Reply};
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
                    let handle = ctx.handle();
                    tracker.spawn(async move { connect(handle, stream, peer).await });
                }
                Err(e) => accept_failed(&mut tracker, e).await,
            },
            finished = tracker.next_finished() => tracker.finished(finished),
        }
    }
}

async fn connect(handle: Arc<SshHandle>, mut stream: TcpStream, peer: SocketAddr) -> Result<()> {
    let request = socks::accept(&mut stream).await?;
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
