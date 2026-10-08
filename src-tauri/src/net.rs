//! Outgoing TCP connections for sessions: the first hop of an SSH connection, and Telnet.
//! Proxies (M14) are meant to plug in here.

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::error::Error;
use crate::forward::host_port;

/// A connection to a server, whatever carries it (TCP, a channel through a jump host).
pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Connects to `host:port`, giving up after 15 seconds.
pub async fn connect(host: &str, port: u16) -> Result<TcpStream> {
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
