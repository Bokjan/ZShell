//! An SSH server in the test process and connections to it, for the tests of what runs on a
//! connection: shells here, port forwarding in `crate::forward` and `crate::connections`. The
//! client is ZShell's own (`ClientHandler`), taking the server's key without reading the
//! user's known_hosts.

use std::sync::Arc;

use russh::client;
use tokio::sync::{mpsc, watch};

use super::handler::ClientHandler;
use super::{Connection, JumpChain, RemoteRoutes, TEST_KEY};
use crate::model::{Remote, SshProfile};

/// A connection of saved session `profile_id` (a quick connection if empty) to `server`,
/// which runs on a stream in this process and lets anyone in without authenticating.
pub async fn connect<H: russh::server::Handler + Send + 'static>(server: H, profile_id: &str) -> Arc<Connection> {
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
    let mut profile = SshProfile::quick("test".into(), Remote::new("localhost".into(), 22, "alice".into()));
    profile.id = profile_id.into();
    let routes = RemoteRoutes::default();
    let (disconnect_tx, disconnect) = watch::channel(None);
    // Nothing is asked: any host key is taken.
    let (queries, _) = mpsc::channel(1);
    let handler = ClientHandler::new("localhost".into(), 22, false, queries, routes.clone(), disconnect_tx).with_any_host_key();
    let mut handle = client::connect_stream(Arc::new(client::Config::default()), client_side, handler).await.unwrap();
    assert!(handle.authenticate_none("alice").await.unwrap().success());
    Connection::new(handle, profile, JumpChain::default(), routes, disconnect)
}
