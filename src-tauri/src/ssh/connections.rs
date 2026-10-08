//! Live SSH connections, keyed by the terminal sessions that use them, so other features
//! (SFTP, port forwarding) can open extra channels on the same connection. A duplicated tab
//! runs its shell on its source tab's connection, which stays open until the last session
//! using it closes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use russh::{client, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::sync::{watch, OnceCell};

use super::handler::ClientHandler;
use crate::error::{Error, Result};
use crate::forward::{Forwards, RemoteRoutes};
use crate::session::{SessionId, SessionSink};

pub type SshHandle = client::Handle<ClientHandler>;

pub struct Connection {
    handle: Arc<SshHandle>,
    /// Connections to the jump hosts this one runs through, first hop first.
    jumps: Vec<Arc<SshHandle>>,
    sftp: OnceCell<Arc<SftpSession>>,
    forwards: Forwards,
    /// Why the connection ended, once it has (see `ClientHandler`).
    disconnect: watch::Receiver<Option<String>>,
}

impl Connection {
    pub fn handle(&self) -> &SshHandle {
        &self.handle
    }

    pub fn disconnect_reason(&self) -> watch::Receiver<Option<String>> {
        self.disconnect.clone()
    }

    pub fn forwards(&self) -> &Forwards {
        &self.forwards
    }

    /// The connection's SFTP session, opened on first use.
    pub async fn sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        self.sftp
            .get_or_try_init(|| async {
                let channel = self.handle.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                let sftp = SftpSession::new(channel.into_stream()).await.context(Error::new("sftp.unsupported"))?;
                Ok(Arc::new(sftp))
            })
            .await
            .cloned()
    }
}

#[derive(Clone, Default)]
pub struct Connections(Arc<Mutex<HashMap<SessionId, Arc<Connection>>>>);

impl Connections {
    /// `routes` and `disconnect` must be the ones given to the connection's
    /// [`ClientHandler`]; forward states are reported through `sink`.
    pub fn insert(
        &self,
        id: SessionId,
        handle: Arc<SshHandle>,
        jumps: Vec<Arc<SshHandle>>,
        routes: RemoteRoutes,
        disconnect: watch::Receiver<Option<String>>,
        sink: SessionSink,
    ) -> Arc<Connection> {
        let forwards = Forwards::new(handle.clone(), routes);
        let connection = Arc::new(Connection { handle, jumps, sftp: OnceCell::new(), forwards, disconnect });
        self.attach(id, connection.clone(), sink);
        connection
    }

    /// Registers session `id` as another user of `connection` (a duplicated tab).
    pub fn attach(&self, id: SessionId, connection: Arc<Connection>, sink: SessionSink) {
        connection.forwards.attach(id, sink);
        self.0.lock().unwrap().insert(id, connection);
    }

    pub fn get(&self, id: SessionId) -> Result<Arc<Connection>> {
        self.0.lock().unwrap().get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))
    }

    /// Unregisters session `id`; if no other session uses its connection, disconnects it in
    /// the background. Idempotent.
    pub fn close(&self, id: SessionId) {
        let connection = {
            let mut connections = self.0.lock().unwrap();
            let Some(connection) = connections.remove(&id) else {
                return;
            };
            connection.forwards.detach(id);
            if connections.values().any(|other| Arc::ptr_eq(other, &connection)) {
                return;
            }
            connection
        };
        connection.forwards.stop_all();
        tauri::async_runtime::spawn(async move {
            if let Some(sftp) = connection.sftp.get() {
                let _ = sftp.close().await;
            }
            let _ = connection.handle.disconnect(Disconnect::ByApplication, "", "en").await;
            for jump in connection.jumps.iter().rev() {
                let _ = jump.disconnect(Disconnect::ByApplication, "", "en").await;
            }
        });
    }
}
