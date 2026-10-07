//! Live SSH connections, keyed by the terminal session that owns them, so other features
//! (SFTP, port forwarding) can open extra channels on the same connection.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use russh::{client, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::sync::OnceCell;

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
}

impl Connection {
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
    /// `routes` must be the table given to the connection's [`ClientHandler`]; forward
    /// states are reported through `sink`.
    pub fn insert(
        &self,
        id: SessionId,
        handle: Arc<SshHandle>,
        jumps: Vec<Arc<SshHandle>>,
        routes: RemoteRoutes,
        sink: SessionSink,
    ) -> Arc<Connection> {
        let forwards = Forwards::new(handle.clone(), routes, sink);
        let connection = Arc::new(Connection { handle, jumps, sftp: OnceCell::new(), forwards });
        self.0.lock().unwrap().insert(id, connection.clone());
        connection
    }

    pub fn get(&self, id: SessionId) -> Result<Arc<Connection>> {
        self.0.lock().unwrap().get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))
    }

    /// Unregisters the connection and disconnects it in the background. Idempotent.
    pub fn close(&self, id: SessionId) {
        let Some(connection) = self.0.lock().unwrap().remove(&id) else {
            return;
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
