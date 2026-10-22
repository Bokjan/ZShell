//! An SSH server in the test process and connections to it, for the tests of what runs on a
//! connection: shells here, port forwarding in `crate::forward` and `crate::connections`. The
//! client is ZShell's own (`ClientHandler`), taking the server's key without reading the
//! user's known_hosts.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// A connection of saved session `profile_id` (a quick connection if empty) to `server`,
/// which runs on a stream in this process and lets anyone in without authenticating.
pub async fn connect<H: russh::server::Handler + Send + 'static>(server: H, profile_id: &str) -> Arc<Connection> {
    connect_with(server, profile_id, None).await.0
}

/// A connection to `server` sending keepalives every `keepalive`, whose transport stalls once
/// told to: as one to a peer gone silently, it then neither reads nor writes any more.
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
    let client_side = Stalling { inner: client_side, stall: stall.clone() };
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
    (Connection::new(handle, profile, JumpChain::default(), routes, disconnect), stall)
}

/// Stalls the transport of a connection made by [`connect_stalling`].
#[derive(Clone, Default)]
pub struct Stall(Arc<AtomicBool>);

impl Stall {
    pub fn now(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn stalled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// A stream that is pending for good once stalled, and wakes nobody.
struct Stalling<S> {
    inner: S,
    stall: Stall,
}

impl<S: AsyncRead + Unpin> AsyncRead for Stalling<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        if self.stall.stalled() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Stalling<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        if self.stall.stalled() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.stall.stalled() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
