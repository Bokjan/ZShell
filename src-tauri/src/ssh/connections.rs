//! Live SSH connections, keyed by the terminal session that owns them, so other features
//! (SFTP now, port forwarding later) can open extra channels on the same connection.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use russh::{client, Disconnect};
use russh_sftp::client::SftpSession;
use tokio::sync::OnceCell;

use super::host_key::ClientHandler;
use crate::error::{Error, Result};
use crate::session::SessionId;

pub type SshHandle = client::Handle<ClientHandler>;

pub struct Connection {
    handle: Arc<SshHandle>,
    sftp: OnceCell<Arc<SftpSession>>,
}

impl Connection {
    /// The connection's SFTP session, opened on first use.
    pub async fn sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        self.sftp
            .get_or_try_init(|| async {
                let channel = self.handle.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                let sftp = SftpSession::new(channel.into_stream()).await.context("服务器不支持 SFTP")?;
                Ok(Arc::new(sftp))
            })
            .await
            .cloned()
    }
}

#[derive(Clone, Default)]
pub struct Connections(Arc<Mutex<HashMap<SessionId, Arc<Connection>>>>);

impl Connections {
    pub fn insert(&self, id: SessionId, handle: Arc<SshHandle>) {
        let connection = Connection { handle, sftp: OnceCell::new() };
        self.0.lock().unwrap().insert(id, Arc::new(connection));
    }

    pub fn get(&self, id: SessionId) -> Result<Arc<Connection>> {
        self.0.lock().unwrap().get(&id).cloned().ok_or(Error::NotConnected(id))
    }

    /// Unregisters the connection and disconnects it in the background. Idempotent.
    pub fn close(&self, id: SessionId) {
        let Some(connection) = self.0.lock().unwrap().remove(&id) else {
            return;
        };
        tauri::async_runtime::spawn(async move {
            if let Some(sftp) = connection.sftp.get() {
                let _ = sftp.close().await;
            }
            let _ = connection.handle.disconnect(Disconnect::ByApplication, "", "en").await;
        });
    }
}
