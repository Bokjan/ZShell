//! An SSH server in the test process and connections to it, for the tests of what runs on a
//! connection: shells here, port forwarding in `crate::forward` and `crate::connections`, SFTP
//! channels in `crate::sftp`. The client is ZShell's own (`ClientHandler`), taking the server's
//! key without reading the user's known_hosts.

use std::pin::Pin;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use russh::client;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, watch};

use super::handler::ClientHandler;
use super::{Connection, JumpChain, RemoteRoutes, TEST_KEY};
use crate::model::{Remote, SshProfile};

/// A server that lets anyone in without authenticating, and does nothing else.
pub struct Anyone;

impl russh::server::Handler for Anyone {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }
}

/// A server that lets anyone in and opens every `direct-tcpip` channel, reading none.
#[derive(Default)]
pub struct Accepting(Vec<russh::Channel<russh::server::Msg>>);

impl russh::server::Handler for Accepting {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        _host: &str,
        _port: u32,
        _originator: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        self.0.push(channel);
        Ok(())
    }
}

/// A server that lets anyone in and opens every `direct-tcpip` channel, sending back what
/// comes through it.
pub struct Echoing;

impl russh::server::Handler for Echoing {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        _host: &str,
        _port: u32,
        _originator: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        tokio::spawn(async move {
            let (mut reader, mut writer) = tokio::io::split(channel.into_stream());
            let _ = tokio::io::copy(&mut reader, &mut writer).await;
        });
        Ok(())
    }
}

/// A server that lets anyone in and is slow to start SFTP, as one that is up but stuck: it
/// opens session channels only after a while (or never), and never answers a request to start
/// a subsystem on them. Counts the channels the client closes.
pub struct Unanswering {
    open: Option<Duration>,
    closed: Arc<AtomicUsize>,
    pending: Vec<russh::server::ChannelOpenHandle>,
    channels: Vec<russh::Channel<russh::server::Msg>>,
}

impl Unanswering {
    /// Opening each channel after `open`, never if `None`; and what counts the channels closed.
    pub fn new(open: Option<Duration>) -> (Self, Arc<AtomicUsize>) {
        let closed = Arc::new(AtomicUsize::new(0));
        (Self { open, closed: closed.clone(), pending: Vec::new(), channels: Vec::new() }, closed)
    }
}

impl russh::server::Handler for Unanswering {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        match self.open {
            None => self.pending.push(reply),
            Some(after) => {
                self.channels.push(channel);
                tokio::spawn(async move {
                    tokio::time::sleep(after).await;
                    reply.accept().await;
                });
            }
        }
        Ok(())
    }

    async fn channel_close(&mut self, _channel: russh::ChannelId, _session: &mut russh::server::Session) -> Result<(), Self::Error> {
        self.closed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A connection of saved session `profile_id` (a quick connection if empty) to `server`,
/// which runs on a stream in this process and lets anyone in without authenticating.
pub async fn connect<H: russh::server::Handler + Send + 'static>(server: H, profile_id: &str) -> Arc<Connection> {
    connect_with(server, profile_id, None).await.0
}

/// A connection to `server` sending keepalives every `keepalive`, whose transport stalls once
/// told to: as one to a peer gone silently, it then neither reads nor writes any more. Or it
/// slows down, as on a slow link.
pub async fn connect_stalling<H: russh::server::Handler + Send + 'static>(server: H, keepalive: Duration) -> (Arc<Connection>, Stall) {
    connect_with(server, "", Some(keepalive)).await
}

async fn connect_with<H: russh::server::Handler + Send + 'static>(
    server: H,
    profile_id: &str,
    keepalive: Option<Duration>,
) -> (Arc<Connection>, Stall) {
    let config = russh::server::Config {
        methods: russh::MethodSet::from(&[russh::MethodKind::None][..]),
        keys: vec![russh::keys::PrivateKey::from_openssh(TEST_KEY).unwrap()],
        ..Default::default()
    };
    let (client_side, server_side) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        if let Ok(session) = russh::server::run_stream(Arc::new(config), server_side, server).await {
            let _ = session.await;
        }
    });
    let stall = Stall::default();
    let (client_side, guard) =
        super::abortable::abortable(Stalling { inner: client_side, stall: stall.clone(), rested: false }, super::stall_limit(keepalive));
    let mut profile = SshProfile::quick("test".into(), Remote::new("localhost".into(), 22, "alice".into()));
    profile.id = profile_id.into();
    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    // Nothing is asked: any host key is taken.
    let (queries, _) = mpsc::channel(1);
    let handler = ClientHandler::new("localhost".into(), 22, false, queries, routes.clone(), disconnect_tx).with_any_host_key();
    let config = Arc::new(super::client_config(keepalive));
    let mut handle = client::connect_stream(config, client_side, handler).await.unwrap();
    assert!(handle.authenticate_none("alice").await.unwrap().success());
    let info = crate::traffic::ConnectionInfo::next(None);
    (Connection::new(handle, guard.keep(), info, profile, JumpChain::default(), routes, disconnect), stall)
}

/// Stalls or slows the transport of a connection made by [`connect_stalling`].
#[derive(Clone, Default)]
pub struct Stall(Arc<AtomicU8>);

const STALLED: u8 = 1;
const SLOW: u8 = 2;

impl Stall {
    pub fn now(&self) {
        self.0.store(STALLED, Ordering::Relaxed);
    }

    /// Writes 1 KiB at a time from now on, after waiting 10 ms for room each time.
    pub fn slow(&self) {
        self.0.store(SLOW, Ordering::Relaxed);
    }

    fn is(&self, mode: u8) -> bool {
        self.0.load(Ordering::Relaxed) == mode
    }
}

/// A stream that is pending for good once stalled, and wakes nobody; or that writes slowly.
struct Stalling<S> {
    inner: S,
    stall: Stall,
    /// Slow, it has waited for room before this write.
    rested: bool,
}

impl<S: AsyncRead + Unpin> AsyncRead for Stalling<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        if self.stall.is(STALLED) {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Stalling<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        if self.stall.is(STALLED) {
            return Poll::Pending;
        }
        if self.stall.is(SLOW) {
            if !std::mem::replace(&mut self.rested, false) {
                self.rested = true;
                let waker = cx.waker().clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    waker.wake();
                });
                return Poll::Pending;
            }
            let n = buf.len().min(1024);
            return Pin::new(&mut self.inner).poll_write(cx, &buf[..n]);
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.stall.is(STALLED) {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
