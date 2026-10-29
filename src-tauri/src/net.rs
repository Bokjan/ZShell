//! Outgoing connections for sessions: the first hop of an SSH connection, and Telnet. Each
//! goes over TCP or through the proxy of its [`Route`].

use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::error::Error;
use crate::model::host_port;
use crate::model::Proxy;
use crate::model::SshProfile;
use crate::session::TermIo;
use crate::vault::SavedPasswords;

/// A connection to a server, whatever carries it (TCP, a channel through a jump host).
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How a session reaches its host: through its jump hosts, first hop first, and the proxy
/// of the first connection. That proxy is the first jump host's own, else the session's, like
/// OpenSSH, where `ProxyJump` connects to the jump host with that host's `ProxyCommand`.
/// `passwords`: where the saved passwords of the session, its jump hosts and its proxy are
/// read; a quick connection's route has none.
#[derive(Default)]
pub struct Route {
    pub jumps: Vec<SshProfile>,
    pub proxy: Option<Proxy>,
    pub passwords: SavedPasswords,
}

/// Connects to `host:port` directly or through `proxy`, whose saved password is in
/// `passwords`. `user` is the user name for a proxy command's `%r`; `keepalive`, TCP
/// keepalives on the connection to the host or the proxy. Also returns the port of that TCP
/// connection on this computer, unless a proxy command carries it.
pub async fn connect(
    host: &str,
    port: u16,
    user: &str,
    proxy: Option<&Proxy>,
    passwords: &SavedPasswords,
    keepalive: Option<Duration>,
    io: &mut TermIo,
) -> Result<(Box<dyn Stream>, Option<u16>)> {
    if let Some(proxy) = proxy {
        return crate::proxy::connect(proxy, host, port, user, passwords, keepalive, io).await;
    }
    let stream = tcp(host, port).await?;
    if let Some(interval) = keepalive {
        set_keepalive(&stream, interval);
    }
    let local_port = local_port(&stream);
    Ok((Box::new(stream), local_port))
}

/// The port of `stream` on this computer.
pub fn local_port(stream: &TcpStream) -> Option<u16> {
    stream.local_addr().ok().map(|address| address.port())
}

/// Opens a TCP connection to `host:port`, trying its addresses in turn, each for up to 15
/// seconds (like OpenSSH's `ConnectTimeout`): a host whose first address doesn't answer (IPv6
/// broken on this network, a dead server among several) is still reached through the next.
/// Looking the name up has the same limit.
pub async fn tcp(host: &str, port: u16) -> Result<TcpStream> {
    let target = host_port(host, port);
    let started = Instant::now();
    let addresses: Vec<SocketAddr> = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((host, port)))
        .await
        .map_err(|_| {
            log::debug!("looking up {host} gave no answer in {CONNECT_TIMEOUT:?}");
            Error::new("net.connectTimeout").param("target", &target)
        })?
        .inspect_err(|e| log::debug!("can't look up {host}: {e}"))
        .context(Error::new("net.connectFailed").param("target", &target))?
        .collect();
    log::debug!("{host} is at {addresses:?} (looked up in {} ms)", started.elapsed().as_millis());
    let started = Instant::now();
    let stream = connect_any(&addresses, CONNECT_TIMEOUT, &target, TcpStream::connect).await?;
    let peer = stream.peer_addr().map_or_else(|_| target.clone(), |peer| peer.to_string());
    let local = local_port(&stream).map_or_else(|| "?".to_owned(), |port| port.to_string());
    log::debug!("TCP connection to {peer} from local port {local} in {} ms", started.elapsed().as_millis());
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Connects with `connect` to the first of `addresses` that answers within `timeout`; fails
/// as the last one did.
async fn connect_any<S, F, Fut>(addresses: &[SocketAddr], timeout: Duration, target: &str, connect: F) -> Result<S>
where
    F: Fn(SocketAddr) -> Fut,
    Fut: Future<Output = std::io::Result<S>>,
{
    let mut last = None;
    for &address in addresses {
        match tokio::time::timeout(timeout, connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => {
                log::debug!("can't connect to {address}: {e}");
                last = Some(anyhow::Error::new(e).context(Error::new("net.connectFailed").param("target", target)));
            }
            Err(_) => {
                log::debug!("{address} gave no answer in {timeout:?}");
                last = Some(Error::new("net.connectTimeout").param("target", target).into());
            }
        }
    }
    Err(last.unwrap_or_else(|| Error::new("net.connectFailed").param("target", target).into()))
}

/// Turns on TCP keepalives every `interval`, so that a peer that went away silently is
/// noticed after three unanswered probes (like SSH's keepalives). Best effort.
pub fn set_keepalive(stream: &TcpStream, interval: Duration) {
    let keepalive = socket2::TcpKeepalive::new().with_time(interval).with_interval(interval).with_retries(3);
    if let Err(e) = socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive) {
        log::warn!("can't turn on TCP keepalives: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An address that doesn't answer in time is given up for the next one, and the error
    /// is the last address's.
    #[tokio::test]
    async fn an_address_that_does_not_answer_is_skipped() {
        let silent: SocketAddr = "192.0.2.1:22".parse().unwrap();
        let refused: SocketAddr = "192.0.2.2:22".parse().unwrap();
        let open: SocketAddr = "192.0.2.3:22".parse().unwrap();
        let connect = |address: SocketAddr| async move {
            match address {
                a if a == silent => std::future::pending().await,
                a if a == refused => Err(std::io::ErrorKind::ConnectionRefused.into()),
                a => Ok(a),
            }
        };
        let timeout = Duration::from_millis(50);
        let code = |result: Result<SocketAddr>| result.unwrap_err().downcast_ref::<Error>().map(Error::code);

        assert_eq!(connect_any(&[silent, refused, open], timeout, "test", connect).await.unwrap(), open);
        assert_eq!(code(connect_any(&[refused, silent], timeout, "test", connect).await), Some("net.connectTimeout"));
        assert_eq!(code(connect_any(&[silent, refused], timeout, "test", connect).await), Some("net.connectFailed"));
        assert_eq!(code(connect_any(&[], timeout, "test", connect).await), Some("net.connectFailed"));
    }
}
