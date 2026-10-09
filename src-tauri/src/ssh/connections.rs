//! Live SSH connections, keyed by the terminal sessions that use them, so other features
//! (SFTP, port forwarding) can open extra channels on the same connection. A duplicated tab
//! runs its shell on its source tab's connection, which stays open until the last session
//! using it closes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use russh::{client, Disconnect};
use russh_sftp::client::{Config as SftpConfig, SftpSession};
use tokio::sync::{watch, OnceCell};

use super::handler::ClientHandler;
use super::JumpChain;
use crate::config::Profile;
use crate::error::{Error, Result};
use crate::forward::{Forwards, RemoteRoutes};
use crate::session::{SessionId, SessionSink};

pub type SshHandle = client::Handle<ClientHandler>;

/// How long an SFTP request may wait for its reply. Generous, since on a slow link a request
/// waits behind the reads queued before it: russh-sftp's default of 10 s failed downloads
/// below about 400 KB/s. A lost connection doesn't wait for it: once the keepalives (or TCP)
/// end the connection, the channel closes and the waiting requests fail.
const SFTP_REQUEST_TIMEOUT_SECS: u64 = 120;
/// Reads kept in flight per file. The SSH channel window (2 MiB) already limits what is on
/// the way, so more only makes later requests (listing a folder) queue longer.
const SFTP_CONCURRENT_READS: usize = 8;

pub struct Connection {
    handle: Arc<SshHandle>,
    /// The profile as it was when connecting; shells opened later (duplicated tabs) use its
    /// terminal options and encoding.
    profile: Profile,
    /// Connections to the jump hosts this one runs through; disconnected once this
    /// connection is dropped.
    _jumps: JumpChain,
    sftp: OnceCell<Arc<SftpSession>>,
    forwards: Forwards,
    /// Why the connection ended, once it has (see `ClientHandler`).
    disconnect: watch::Receiver<Option<String>>,
}

impl Connection {
    pub fn handle(&self) -> &SshHandle {
        &self.handle
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
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
                let stream = crate::sftp::names::convert(channel.into_stream(), crate::encoding::for_profile(&self.profile.encoding));
                let config = SftpConfig {
                    request_timeout_secs: SFTP_REQUEST_TIMEOUT_SECS,
                    max_concurrent_reads: SFTP_CONCURRENT_READS,
                    ..Default::default()
                };
                let sftp = SftpSession::new_with_config(stream, config).await.context(Error::new("sftp.unsupported"))?;
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
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &self,
        id: SessionId,
        handle: Arc<SshHandle>,
        profile: Profile,
        jumps: JumpChain,
        routes: RemoteRoutes,
        disconnect: watch::Receiver<Option<String>>,
        sink: SessionSink,
    ) -> Arc<Connection> {
        let forwards = Forwards::new(handle.clone(), routes);
        let connection = Arc::new(Connection { handle, profile, _jumps: jumps, sftp: OnceCell::new(), forwards, disconnect });
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
        });
    }
}
