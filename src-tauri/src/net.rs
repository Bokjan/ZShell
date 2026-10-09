//! Outgoing connections for sessions: the first hop of an SSH connection, and Telnet. Each
//! goes over TCP or through the proxy of its [`Route`].

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::config::Profile;
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
    pub jumps: Vec<Profile>,
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

/// Opens a TCP connection to `host:port`, giving up after 15 seconds.
pub async fn tcp(host: &str, port: u16) -> Result<TcpStream> {
    let target = host_port(host, port);
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| Error::new("net.connectTimeout").param("target", &target))?
        .context(Error::new("net.connectFailed").param("target", &target))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Turns on TCP keepalives every `interval`, so that a peer that went away silently is
/// noticed after three unanswered probes (like SSH's keepalives). Best effort.
pub fn set_keepalive(stream: &TcpStream, interval: Duration) {
    let keepalive = socket2::TcpKeepalive::new().with_time(interval).with_interval(interval).with_retries(3);
    let _ = socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive);
}
