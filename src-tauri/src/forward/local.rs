//! `-L`: listen on this computer; each connection is carried to the target through a
//! `direct-tcpip` channel opened from the server.

use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::task::Poll;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpSocket, TcpStream};

use super::{bridge, host_port, Ctx, ForwardRule, InFlight, Opener, Stopping, Tracker, LOOKUP_TIMEOUT};
use crate::error::Error;
use crate::model::ALL_ADDRESSES;

pub async fn run(rule: &ForwardRule, ctx: &Ctx, stopping: &mut Stopping) -> Result<InFlight> {
    serve(rule, ctx, stopping, |stream, peer| connect(ctx.opener(), stream, peer, rule.target_host.clone(), rule.target_port)).await
}

/// Listens where `rule` says until it is asked to stop, carrying each connection accepted on
/// with what `carry` makes of it (a local rule's or a dynamic one's), and reporting them.
/// Gives back the connections it carries if it was released.
pub(super) async fn serve<F>(
    rule: &ForwardRule,
    ctx: &Ctx,
    stopping: &mut Stopping,
    carry: impl Fn(TcpStream, SocketAddr) -> F,
) -> Result<InFlight>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    let listener = tokio::select! {
        listener = Listener::bind(&rule.bind_host, rule.bind_port) => listener?,
        () = stopping.requested() => return Ok(InFlight::default()),
    };
    let mut tracker = Tracker::new(ctx, listener.address(&rule.bind_host)?);
    loop {
        let report = tracker.report_due();
        tokio::select! {
            () = super::at(report) => tracker.report(),
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => tracker.spawn(carry(stream, peer)),
                Err(e) => accept_failed(&mut tracker, e).await,
            },
            finished = tracker.next_finished() => tracker.finished(finished),
            () = stopping.requested() => return Ok(tracker.into_in_flight(stopping)),
        }
    }
}

/// Where a local or dynamic rule listens: every address its bind host resolves to, as
/// OpenSSH does, so a rule on `localhost` takes connections to both 127.0.0.1 and ::1
/// (clients differ in which one they try); IPv4's and IPv6's any address for `*`.
pub(super) struct Listener(Vec<TcpListener>);

impl Listener {
    /// Fails only if no address could be listened on.
    pub(super) async fn bind(host: &str, port: u16) -> Result<Self> {
        let failed = || Error::new("forward.bindFailed").param("address", host_port(host, port));
        let mut addresses: Vec<SocketAddr> = Vec::new();
        let found = match host {
            ALL_ADDRESSES => Ok(vec![(Ipv4Addr::UNSPECIFIED, port).into(), (Ipv6Addr::UNSPECIFIED, port).into()]),
            _ => tokio::time::timeout(LOOKUP_TIMEOUT, tokio::net::lookup_host((host, port)))
                .await
                .unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()))
                .map(Vec::from_iter),
        };
        for address in found.with_context(failed)? {
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
            match listen(address) {
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

/// A listener on `address`. One on IPv6 takes only IPv6 connections, as OpenSSH's do: where
/// the system would also give it IPv4 ones (Linux), it would take the port from the IPv4
/// listener beside it.
fn listen(address: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = match address {
        SocketAddr::V4(_) => TcpSocket::new_v4()?,
        SocketAddr::V6(_) => {
            let socket = TcpSocket::new_v6()?;
            socket2::SockRef::from(&socket).set_only_v6(true)?;
            socket
        }
    };
    // As `TcpListener::bind` does: a port whose last connections are closing can be taken again.
    #[cfg(unix)]
    socket.set_reuseaddr(true)?;
    socket.bind(address)?;
    socket.listen(1024)
}

/// Accept errors are usually transient (aborted handshakes, descriptor exhaustion); report
/// and keep listening, pausing briefly so a persistent error cannot spin.
async fn accept_failed(tracker: &mut Tracker<'_>, error: std::io::Error) {
    tracker.failed(Error::new("forward.acceptFailed").detail(error));
    tokio::time::sleep(Duration::from_millis(100)).await;
}

async fn connect(opener: Opener, stream: TcpStream, peer: SocketAddr, host: String, port: u16) -> Result<()> {
    let channel = opener.open(host, port, peer).await?;
    bridge(stream, channel, opener.liveness).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

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

    #[tokio::test]
    async fn listens_on_ipv4_and_ipv6_for_every_address() {
        let listener = Listener::bind(ALL_ADDRESSES, 0).await.unwrap();
        let bound: Vec<SocketAddr> = listener.0.iter().map(|l| l.local_addr().unwrap()).collect();
        let port = bound[0].port();
        assert_eq!(bound, [SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)), SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))]);
        assert_eq!(listener.address(ALL_ADDRESSES).unwrap(), format!("*:{port}"));
        for ip in [IpAddr::from(Ipv4Addr::LOCALHOST), IpAddr::from(Ipv6Addr::LOCALHOST)] {
            let _client = TcpStream::connect((ip, port)).await.unwrap();
            let (_, peer) = listener.accept().await.unwrap();
            assert_eq!(peer.ip(), ip);
        }
    }
}
