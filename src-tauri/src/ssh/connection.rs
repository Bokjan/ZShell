//! An authenticated SSH connection: what the session's shell, the shells of duplicated tabs
//! and the other features (SFTP, port forwarding; see `crate::connections`, which keeps them
//! by session) open their channels on.

use std::sync::{Arc, Mutex};

use russh::client;
use tokio::sync::watch;

use super::abortable::Link;
use super::forwarded::RemoteRoutes;
use super::handler::{ClientHandler, EndReason};
use super::liveness::Liveness;
use super::JumpChain;
use crate::model::SshProfile;
use crate::session::{SessionId, TermIo};

pub type SshHandle = client::Handle<ClientHandler>;

pub struct Connection {
    handle: Arc<SshHandle>,
    /// The profile as it was when connecting; shells opened later (duplicated tabs) use its
    /// terminal options and encoding.
    profile: SshProfile,
    /// Connections to the jump hosts this one runs through; disconnected once this
    /// connection is dropped.
    _jumps: JumpChain,
    /// Where the channels the server opens for remote forwards go.
    routes: RemoteRoutes,
    /// Why the connection ended, once it has (see `ClientHandler`).
    disconnect: watch::Receiver<Option<EndReason>>,
    /// Its tabs' shells.
    shells: Arc<Shells>,
    liveness: Liveness,
}

impl Connection {
    /// `routes` and `disconnect` must be the ones given to the connection's [`ClientHandler`],
    /// and `link` the one of its transport.
    pub(super) fn new(
        handle: SshHandle,
        link: Link,
        profile: SshProfile,
        jumps: JumpChain,
        routes: RemoteRoutes,
        disconnect: watch::Receiver<Option<EndReason>>,
    ) -> Arc<Self> {
        let handle = Arc::new(handle);
        let liveness = Liveness::new(link, &handle, profile.ssh.remote.keepalive().is_some());
        Arc::new(Self { handle, profile, _jumps: jumps, routes, disconnect, shells: Arc::default(), liveness })
    }

    pub fn handle(&self) -> &Arc<SshHandle> {
        &self.handle
    }

    pub fn profile(&self) -> &SshProfile {
        &self.profile
    }

    pub fn routes(&self) -> &RemoteRoutes {
        &self.routes
    }

    pub fn disconnect_reason(&self) -> watch::Receiver<Option<EndReason>> {
        self.disconnect.clone()
    }

    pub fn shells(&self) -> &Shells {
        &self.shells
    }

    /// Checks that the connection is still alive (see [`Liveness`]).
    pub fn liveness(&self) -> &Liveness {
        &self.liveness
    }

    /// Notes that session `id` opens its shell now, until the session closes.
    pub(super) fn shell_opened(&self, id: SessionId, io: &TermIo) {
        self.shells.opened(id);
        let shells = self.shells.clone();
        io.on_close(move || shells.closed(id));
    }
}

/// The sessions (panes) whose shells are open on a connection, oldest first: the order the
/// server started their processes in (see `crate::sftp::shell_dir`).
#[derive(Default)]
pub struct Shells(Mutex<ShellOrder>);

#[derive(Default)]
struct ShellOrder {
    open: Vec<SessionId>,
    /// Counts the sessions opened and closed, for a search to tell whether a place in `open`
    /// still holds.
    changes: u64,
}

impl Shells {
    /// Session `id` is opening a shell, after those of the sessions open now.
    pub fn opened(&self, id: SessionId) {
        let mut order = self.0.lock().unwrap();
        order.open.push(id);
        order.changes += 1;
    }

    pub fn closed(&self, id: SessionId) {
        let mut order = self.0.lock().unwrap();
        order.open.retain(|open| *open != id);
        order.changes += 1;
    }

    /// The sessions whose shells are open, oldest first, and the count of changes so far.
    pub fn open(&self) -> (Vec<SessionId>, u64) {
        let order = self.0.lock().unwrap();
        (order.open.clone(), order.changes)
    }

    /// The count of changes so far (see [`Shells::open`]).
    pub fn changes(&self) -> u64 {
        self.0.lock().unwrap().changes
    }
}
