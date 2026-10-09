//! Live SSH connections, keyed by the terminal sessions that use them, so other features
//! (SFTP, port forwarding) can open extra channels on the same connection. A duplicated tab
//! runs its shell on its source tab's connection, which stays open until the last session
//! using it closes.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use anyhow::Context;
use russh::{client, Disconnect};
use russh_sftp::client::{Config as SftpConfig, SftpSession};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::watch;

use super::handler::ClientHandler;
use super::JumpChain;
use crate::config::SshProfile;
use crate::error::{Error, Result};
use crate::forward::{ForwardRule, Forwards, Hub, RemoteRoutes};
use crate::session::{SessionId, TermIo};

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
    profile: SshProfile,
    /// Connections to the jump hosts this one runs through; disconnected once this
    /// connection is dropped.
    _jumps: JumpChain,
    /// The SFTP session for browsing and editing, once opened...
    sftp: SftpSlot,
    /// ...and the one for transfers (see [`Connection::transfer_sftp`]).
    transfer_sftp: SftpSlot,
    forwards: Forwards,
    /// Whether its forwards move to another connection of the session when it closes (the
    /// user chose so when closing its last tab).
    keep_forwards: AtomicBool,
    /// Why the connection ended, once it has (see `ClientHandler`).
    disconnect: watch::Receiver<Option<String>>,
}

impl Connection {
    pub fn handle(&self) -> &SshHandle {
        &self.handle
    }

    pub fn profile(&self) -> &SshProfile {
        &self.profile
    }

    pub fn disconnect_reason(&self) -> watch::Receiver<Option<String>> {
        self.disconnect.clone()
    }

    /// The connection's SFTP session for browsing (and editing), opened on first use.
    pub async fn sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        self.open_sftp(&self.sftp).await
    }

    /// The SFTP session for transfers (uploads, downloads, drags out of the window), on a
    /// channel of its own: requests on one channel are answered in order, so a folder
    /// listed during a large transfer would wait behind the transfer's reads. The server
    /// may refuse another channel (`MaxSessions`); transfers then share the browsing one.
    pub async fn transfer_sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        match self.open_sftp(&self.transfer_sftp).await {
            Ok(sftp) => Ok(sftp),
            Err(_) => self.sftp().await,
        }
    }

    /// The SFTP session kept in `slot`, opened on first use, and again if its channel ended
    /// while the connection stays (the server's `sftp-server` exited, say): otherwise every
    /// SFTP operation would fail until the tab reconnects.
    async fn open_sftp(&self, slot: &SftpSlot) -> anyhow::Result<Arc<SftpSession>> {
        let mut cached = slot.lock().await;
        if let Some((sftp, ended)) = cached.as_ref() {
            if !ended.load(Ordering::Relaxed) {
                return Ok(sftp.clone());
            }
        }
        let channel = self.handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let ended = Arc::new(AtomicBool::new(false));
        let stream = crate::sftp::names::convert(channel.into_stream(), crate::encoding::for_profile(&self.profile.encoding));
        let stream = Watched { inner: stream, ended: ended.clone() };
        let config = SftpConfig {
            request_timeout_secs: SFTP_REQUEST_TIMEOUT_SECS,
            max_concurrent_reads: SFTP_CONCURRENT_READS,
            ..Default::default()
        };
        let sftp = Arc::new(SftpSession::new_with_config(stream, config).await.context(Error::new("sftp.unsupported"))?);
        *cached = Some((sftp.clone(), ended));
        Ok(sftp)
    }
}

/// An SFTP session, once opened, and whether its channel has ended since.
type SftpSlot = tokio::sync::Mutex<Option<(Arc<SftpSession>, Arc<AtomicBool>)>>;

/// An SFTP session's stream, noting when reading from it ends: russh-sftp doesn't tell.
struct Watched {
    inner: Box<dyn crate::sftp::names::Stream>,
    ended: Arc<AtomicBool>,
}

impl AsyncRead for Watched {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        let eof = matches!(result, Poll::Ready(Ok(()))) && buf.filled().len() == before && buf.remaining() > 0;
        if eof || matches!(result, Poll::Ready(Err(_))) {
            self.ended.store(true, Ordering::Relaxed);
        }
        result
    }
}

impl AsyncWrite for Watched {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Whether two connections belong to the same saved session (quick connections have none).
fn same_session(a: &Connection, b: &Connection) -> bool {
    !a.profile.id.is_empty() && a.profile.id == b.profile.id
}

/// The live connections, by the sessions (tabs) using them.
///
/// A saved session's forwarding rules run once, on one of its connections (tabs opened
/// separately each have their own): starting or stopping a rule from any tab acts where it
/// runs, a new connection doesn't start a rule another one runs, and when a connection closes
/// its rules move to another one if the user chose so.
#[derive(Clone, Default)]
pub struct Connections(Arc<Mutex<HashMap<SessionId, Arc<Connection>>>>);

type Map = HashMap<SessionId, Arc<Connection>>;

/// The distinct connections of `connection`'s session, itself first.
fn session_connections(map: &Map, connection: &Arc<Connection>) -> Vec<Arc<Connection>> {
    let mut list = vec![connection.clone()];
    for other in map.values() {
        if same_session(other, connection) && !list.iter().any(|c| Arc::ptr_eq(c, other)) {
            list.push(other.clone());
        }
    }
    list
}

impl Connections {
    /// Registers a new connection for session `id`, until the session closes. `routes` and
    /// `disconnect` must be the ones given to the connection's [`ClientHandler`]; forward
    /// states are reported to the session's tab.
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &self,
        id: SessionId,
        handle: Arc<SshHandle>,
        profile: SshProfile,
        jumps: JumpChain,
        routes: RemoteRoutes,
        disconnect: watch::Receiver<Option<String>>,
        io: &TermIo,
    ) -> Arc<Connection> {
        let hub = {
            let map = self.0.lock().unwrap();
            let sibling = map.values().find(|other| !profile.id.is_empty() && other.profile.id == profile.id);
            sibling.map(|other| other.forwards.hub()).unwrap_or_else(|| Arc::new(Hub::default()))
        };
        let forwards = Forwards::new(handle.clone(), routes, hub);
        let connection = Arc::new(Connection {
            handle,
            profile,
            _jumps: jumps,
            sftp: SftpSlot::default(),
            transfer_sftp: SftpSlot::default(),
            forwards,
            keep_forwards: AtomicBool::new(false),
            disconnect,
        });
        self.attach(id, connection.clone(), io);
        connection
    }

    /// Registers session `id` as another user of `connection` (a duplicated tab), until the
    /// session closes. A session closing meanwhile is unregistered right away.
    pub fn attach(&self, id: SessionId, connection: Arc<Connection>, io: &TermIo) {
        connection.forwards.attach(id, io.sink());
        self.0.lock().unwrap().insert(id, connection);
        let connections = self.clone();
        io.on_close(move || connections.close(id));
    }

    pub fn get(&self, id: SessionId) -> Result<Arc<Connection>> {
        self.0.lock().unwrap().get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))
    }

    /// Unregisters session `id`; if no other session uses its connection, disconnects it in
    /// the background. Idempotent.
    fn close(&self, id: SessionId) {
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
        let (rules, tasks) = connection.forwards.stop_all();
        let handover = connection.keep_forwards.load(Ordering::Relaxed) && !rules.is_empty();
        let connections = self.clone();
        tauri::async_runtime::spawn(async move {
            if handover {
                // Their ports (and listeners on the server) are released first.
                for task in tasks {
                    let _ = task.await;
                }
            }
            for slot in [&connection.sftp, &connection.transfer_sftp] {
                if let Some((sftp, _)) = slot.lock().await.take() {
                    let _ = sftp.close().await;
                }
            }
            let _ = connection.handle.disconnect(Disconnect::ByApplication, "", "en").await;
            if handover {
                connections.adopt(&connection, rules);
            }
        });
    }

    /// Starts the rules that start on connecting, and those in `carry` (running before the
    /// tab reconnected), unless another connection of the session runs them.
    pub fn auto_start(&self, connection: &Arc<Connection>, carry: &[String]) {
        let others = session_connections(&self.0.lock().unwrap(), connection).split_off(1);
        for rule in connection.profile.ssh.forwards.iter().filter(|rule| rule.auto_start || carry.contains(&rule.id)) {
            if !others.iter().any(|other| other.forwards.is_running(&rule.id)) {
                connection.forwards.start(rule.clone(), true);
            }
        }
    }

    /// Starts (or restarts with a new definition) a rule of session `id`'s saved session:
    /// where it runs, else on `id`'s connection.
    pub fn start_forward(&self, id: SessionId, rule: ForwardRule) -> Result<()> {
        let target = {
            let map = self.0.lock().unwrap();
            let own = map.get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))?;
            session_connections(&map, &own).into_iter().find(|c| c.forwards.is_running(&rule.id)).unwrap_or(own)
        };
        target.forwards.start(rule, false);
        Ok(())
    }

    /// Stops a rule of session `id`'s saved session wherever it runs (or clears its failure).
    pub fn stop_forward(&self, id: SessionId, rule_id: &str) -> Result<()> {
        let targets = {
            let map = self.0.lock().unwrap();
            let own = map.get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))?;
            let holders: Vec<_> = session_connections(&map, &own).into_iter().filter(|c| c.forwards.has(rule_id)).collect();
            if holders.is_empty() { vec![own] } else { holders }
        };
        for target in targets {
            target.forwards.stop(rule_id);
        }
        Ok(())
    }

    /// Stops the rules of saved session `profile_id` that are no longer among `rule_ids`.
    pub fn retain_forwards(&self, profile_id: &str, rule_ids: &[String]) {
        let connections: Vec<_> = self.0.lock().unwrap().values().filter(|c| c.profile.id == profile_id).cloned().collect();
        for connection in connections {
            for rule in connection.forwards.running() {
                if !rule_ids.contains(&rule.id) {
                    connection.forwards.stop(&rule.id);
                }
            }
        }
    }

    /// The rules that closing sessions `ids` would stop although another connection of their
    /// saved session stays open, which could take them over.
    pub fn forwards_to_keep(&self, ids: &[SessionId]) -> Vec<ForwardRule> {
        let map = self.0.lock().unwrap();
        let mut closing: Vec<Arc<Connection>> = Vec::new();
        for connection in ids.iter().filter_map(|id| map.get(id)) {
            if !closing.iter().any(|c| Arc::ptr_eq(c, connection)) {
                closing.push(connection.clone());
            }
        }
        let mut rules = Vec::new();
        for connection in closing {
            let stays = |other: &Arc<Connection>| map.iter().any(|(id, c)| !ids.contains(id) && Arc::ptr_eq(c, other));
            // Used by a tab that stays open: it doesn't close.
            if stays(&connection) {
                continue;
            }
            if session_connections(&map, &connection).iter().skip(1).any(stays) {
                rules.extend(connection.forwards.running());
            }
        }
        rules
    }

    /// Moves the forwards of the connections of sessions `ids` to another connection of their
    /// saved session once they close.
    pub fn keep_forwards(&self, ids: &[SessionId]) {
        let map = self.0.lock().unwrap();
        for connection in ids.iter().filter_map(|id| map.get(id)) {
            connection.keep_forwards.store(true, Ordering::Relaxed);
        }
    }

    /// The rules running on session `id`'s connection if it is the connection's last user, to
    /// carry over to the connection it reconnects with.
    pub fn forwards_to_carry(&self, id: SessionId) -> Vec<String> {
        let map = self.0.lock().unwrap();
        let Some(connection) = map.get(&id) else { return Vec::new() };
        if map.iter().any(|(other, c)| *other != id && Arc::ptr_eq(c, connection)) {
            return Vec::new();
        }
        connection.forwards.running().into_iter().map(|rule| rule.id).collect()
    }

    /// Starts `rules` of a closed connection on another connection of its session, if any.
    fn adopt(&self, closed: &Connection, rules: Vec<ForwardRule>) {
        let session = {
            let map = self.0.lock().unwrap();
            map.values().find(|c| same_session(c, closed)).map(|c| session_connections(&map, c))
        };
        let Some(session) = session else { return };
        for rule in rules {
            if !session.iter().any(|c| c.forwards.is_running(&rule.id)) {
                session[0].forwards.start(rule, true);
            }
        }
    }
}
