//! Outgoing connections for sessions: the first hop of an SSH connection, and Telnet. Each
//! goes over TCP or through the proxy of its [`Route`].

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::config::SshProfile;
use crate::error::Error;
use crate::forward::host_port;
use crate::proxy::Proxy;
use crate::session::TermIo;

/// A connection to a server, whatever carries it (TCP, a channel through a jump host).
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How a session reaches its host: through its jump hosts, first hop first, and the proxy
/// of the first connection. That proxy is the first jump host's own, else the session's, like
/// OpenSSH, where `ProxyJump` connects to the jump host with that host's `ProxyCommand`.
#[derive(Default)]
pub struct Route {
    pub jumps: Vec<SshProfile>,
    pub proxy: Option<Proxy>,
}

/// Connects to `host:port` directly or through `proxy`. `user` is the user name for a proxy
/// command's `%r`; `keepalive`, TCP keepalives on the connection to the host or the proxy.
pub async fn connect(
    host: &str,
    port: u16,
    user: &str,
    proxy: Option<&Proxy>,
    keepalive: Option<Duration>,
    io: &mut TermIo,
) -> Result<Box<dyn Stream>> {
    if let Some(proxy) = proxy {
        return crate::proxy::connect(proxy, host, port, user, keepalive, io).await;
    }
    let stream = tcp(host, port).await?;
    if let Some(interval) = keepalive {
        set_keepalive(&stream, interval);
    }
    Ok(Box::new(stream))
}

/// Opens a TCP connection to `host:port`, trying its addresses in turn, each for up to 15
/// seconds (like OpenSSH's `ConnectTimeout`): a host whose first address doesn't answer (IPv6
/// broken on this network, a dead server among several) is still reached through the next.
pub async fn tcp(host: &str, port: u16) -> Result<TcpStream> {
    let target = host_port(host, port);
    let addresses: Vec<SocketAddr> = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((host, port)))
        .await
        .map_err(|_| Error::new("net.connectTimeout").param("target", &target))?
        .context(Error::new("net.connectFailed").param("target", &target))?
        .collect();
    let stream = connect_any(&addresses, CONNECT_TIMEOUT, &target, TcpStream::connect).await?;
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
            Ok(Err(e)) => last = Some(anyhow::Error::new(e).context(Error::new("net.connectFailed").param("target", target))),
            Err(_) => last = Some(Error::new("net.connectTimeout").param("target", target).into()),
        }
    }
    Err(last.unwrap_or_else(|| Error::new("net.connectFailed").param("target", target).into()))
}

/// Turns on TCP keepalives every `interval`, so that a peer that went away silently is
/// noticed after three unanswered probes (like SSH's keepalives). Best effort.
pub fn set_keepalive(stream: &TcpStream, interval: Duration) {
    let keepalive = socket2::TcpKeepalive::new().with_time(interval).with_interval(interval).with_retries(3);
    let _ = socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive);
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
