//! The live SSH connections, keyed by the terminal sessions (tabs) that use them, with what
//! runs on each besides shells: its SFTP sessions and its forwarding rules. A duplicated tab
//! runs its shell on its source tab's connection, which stays open until the last session
//! using it closes.
//!
//! This is where the features meet: `ssh` makes the connection (and knows nothing of what else
//! runs on it), `sftp` and `forward` use it, and the registry decides where a saved session's
//! forwarding rules run and what happens to them when a connection closes, or when the shell
//! of its last tab exits ([`exit`]).

mod exit;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use russh::Disconnect;
use russh_sftp::client::SftpSession;

use crate::error::{Error, Result};
use crate::forward::{Draining, Forwards, Hub};
use crate::model::{ForwardRule, Profile};
use crate::session::{SessionId, TermIo, Viewers};
use crate::sftp::channels::Channels;
use crate::sftp::shell_dir::ShellDirs;
use crate::sftp::transfer::TransferSlots;
use crate::ssh;

/// A live SSH connection and what runs on it besides shells.
pub struct LiveConnection {
    ssh: Arc<ssh::Connection>,
    /// The tabs using it, which its SFTP traffic is reported to.
    viewers: Viewers,
    sftp: Channels,
    /// What has been found of its tabs' shells on the server.
    shell_dirs: ShellDirs,
    forwards: Forwards,
    /// Whether its forwards move to another connection of the session when it closes (the
    /// user chose so when closing its last tab).
    keep_forwards: AtomicBool,
    /// The connections its rules carried when its last shell exited, still open after the tab
    /// started another shell: the next time, they are waited for too.
    draining: Mutex<Draining>,
}

impl LiveConnection {
    pub fn ssh(&self) -> &Arc<ssh::Connection> {
        &self.ssh
    }

    /// The SFTP session for browsing and editing (see [`Channels::browse`]). Each SFTP command
    /// takes it, and its requests are answered: the connection is checked unless an answer
    /// comes soon (see [`ssh::Liveness`]).
    pub async fn sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        self.ssh.liveness().expect_reply();
        self.sftp.browse(&self.ssh).await
    }

    /// How many connections its forwarding carries: those of the rules running here, and those
    /// its rules carried when an earlier shell exited, still open.
    fn forward_connections(&self) -> usize {
        let mut draining = self.draining.lock().unwrap();
        draining.forget_ended();
        self.forwards.connections() + draining.len()
    }

    /// The SFTP session for transfers (see [`Channels::transfer`]), checking the connection as
    /// [`LiveConnection::sftp`] does.
    pub async fn transfer_sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        self.ssh.liveness().expect_reply();
        self.sftp.transfer(&self.ssh).await
    }

    /// What its transfers copy files at once by (see [`Channels::transfer_slots`]).
    pub fn transfer_slots(&self) -> Arc<TransferSlots> {
        self.sftp.transfer_slots()
    }

    /// The folder session `id`'s shell is in, where the server lets it be found (see
    /// [`crate::sftp::shell_dir`]).
    pub async fn shell_dir(&self, id: SessionId) -> anyhow::Result<Option<String>> {
        let sftp = self.sftp().await?;
        Ok(self.shell_dirs.find(sftp.as_ref(), self.ssh.shells(), id).await)
    }
}

/// What deciding where a session's forwarding rules run needs of a connection: a fake one in
/// tests.
trait Holder {
    /// The saved session it belongs to; empty for a quick connection.
    fn session(&self) -> &str;
    /// The rules running on it.
    fn running(&self) -> Vec<ForwardRule>;
    fn is_running(&self, rule_id: &str) -> bool;
}

impl Holder for LiveConnection {
    fn session(&self) -> &str {
        &self.ssh.profile().id
    }
    fn running(&self) -> Vec<ForwardRule> {
        self.forwards.running()
    }
    fn is_running(&self, rule_id: &str) -> bool {
        self.forwards.is_running(rule_id)
    }
}

/// Whether two connections belong to the same saved session (quick connections have none).
fn same_session<C: Holder>(a: &C, b: &C) -> bool {
    !a.session().is_empty() && a.session() == b.session()
}

/// The live connections, by the sessions (tabs) using them.
///
/// A saved session's forwarding rules run once, on one of its connections (tabs opened
/// separately each have their own): starting or stopping a rule from any tab acts where it
/// runs, a new connection doesn't start a rule another one runs, and when a connection closes
/// its rules move to another one if the user chose so.
#[derive(Clone, Default)]
pub struct Connections {
    map: Arc<Mutex<HashMap<SessionId, Arc<LiveConnection>>>>,
    /// The rules that were running on the connections of sessions that have closed, by the
    /// last session using each: the tab reconnects with them (see [`Connections::forwards_to_carry`]).
    /// A few at most: those of tabs closed for good are never asked for.
    carried: Arc<Mutex<VecDeque<Carried>>>,
}

/// A closed session, and the ids of the rules its connection ran.
type Carried = (SessionId, Vec<String>);

/// How many closed sessions' rules [`Connections::carried`] keeps.
const CARRIED: usize = 32;

type Map<C = LiveConnection> = HashMap<SessionId, Arc<C>>;

/// The distinct connections of `connection`'s session, itself first.
fn session_connections<C: Holder>(map: &Map<C>, connection: &Arc<C>) -> Vec<Arc<C>> {
    let mut list = vec![connection.clone()];
    for other in map.values() {
        if same_session(other.as_ref(), connection.as_ref()) && !list.iter().any(|c| Arc::ptr_eq(c, other)) {
            list.push(other.clone());
        }
    }
    list
}

/// Of `rules`, those a new `connection` starts: the automatic ones and those in `carry`,
/// unless another connection of its session runs them.
fn rules_to_start<C: Holder>(map: &Map<C>, connection: &Arc<C>, rules: &[ForwardRule], carry: &[String]) -> Vec<ForwardRule> {
    let others = session_connections(map, connection).split_off(1);
    rules
        .iter()
        .filter(|rule| rule.auto_start || carry.contains(&rule.id))
        .filter(|rule| !others.iter().any(|other| other.is_running(&rule.id)))
        .cloned()
        .collect()
}

/// The rules that closing sessions `ids` would stop although another connection of their
/// saved session stays open, which could take them over.
fn rules_to_keep<C: Holder>(map: &Map<C>, ids: &[SessionId]) -> Vec<ForwardRule> {
    let stays = |other: &Arc<C>| map.iter().any(|(id, c)| !ids.contains(id) && Arc::ptr_eq(c, other));
    let mut rules = Vec::new();
    for connection in closing(map, ids) {
        if session_connections(map, &connection).iter().skip(1).any(stays) {
            rules.extend(connection.running());
        }
    }
    rules
}

/// The sessions (tabs, in any window) whose connection runs forwarding rules of saved session
/// `profile_id`, in order.
fn forwarding_sessions<C: Holder>(map: &Map<C>, profile_id: &str) -> Vec<SessionId> {
    let mut ids: Vec<SessionId> =
        map.iter().filter(|(_, c)| c.session() == profile_id && !c.running().is_empty()).map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids
}

/// The connections that closing sessions `ids` disconnects: those no other tab uses.
fn closing<C>(map: &Map<C>, ids: &[SessionId]) -> Vec<Arc<C>> {
    let mut closing: Vec<Arc<C>> = Vec::new();
    for connection in ids.iter().filter_map(|id| map.get(id)) {
        let stays = map.iter().any(|(id, c)| !ids.contains(id) && Arc::ptr_eq(c, connection));
        if !stays && !closing.iter().any(|c| Arc::ptr_eq(c, connection)) {
            closing.push(connection.clone());
        }
    }
    closing
}

/// The rules running on session `id`'s connection if it is the connection's last user, to
/// carry over to the connection it reconnects with.
fn rules_to_carry<C: Holder>(map: &Map<C>, id: SessionId) -> Vec<String> {
    let Some(connection) = map.get(&id) else { return Vec::new() };
    if map.iter().any(|(other, c)| *other != id && Arc::ptr_eq(c, connection)) {
        return Vec::new();
    }
    connection.running().into_iter().map(|rule| rule.id).collect()
}

/// Where the rules of a closed connection go: another connection of its session (the first
/// found), with those of `rules` that none of them runs.
fn rules_to_adopt<C: Holder>(map: &Map<C>, closed: &C, rules: Vec<ForwardRule>) -> Option<(Arc<C>, Vec<ForwardRule>)> {
    // The connection is still listed while its last tab waits after its shell exited.
    let some = map.values().find(|c| same_session(c.as_ref(), closed) && !std::ptr::eq(c.as_ref(), closed))?;
    let session = session_connections(map, some);
    let rules = rules.into_iter().filter(|rule| !session.iter().any(|c| c.is_running(&rule.id))).collect();
    Some((session[0].clone(), rules))
}

impl Connections {
    /// The system has woken from sleep, and the network may have changed under the
    /// connections: checks that each is still alive (see [`ssh::Liveness`]).
    pub fn woke(&self) {
        for connection in self.map.lock().unwrap().values() {
            connection.ssh.liveness().woke();
        }
    }

    /// Registers the new connection of session `id` until the session closes, and starts the
    /// rules that start on connecting and those in `carry` (running before the tab
    /// reconnected), unless another connection of the session runs them. Forward states are
    /// reported to the session's tab.
    pub fn connected(&self, id: SessionId, ssh: Arc<ssh::Connection>, carry: &[String], io: &TermIo) {
        // Under one lock, so that two tabs of a session connecting at once share a hub, and
        // don't both start a rule (one would find its port taken).
        {
            let mut map = self.map.lock().unwrap();
            let profile_id = &ssh.profile().id;
            let sibling = map.values().find(|other| !profile_id.is_empty() && other.ssh.profile().id == *profile_id);
            let hub = sibling.map(|other| other.forwards.hub()).unwrap_or_else(|| Arc::new(Hub::default()));
            let forwards = Forwards::new(&ssh, hub);
            let viewers = Viewers::default();
            let connection = Arc::new(LiveConnection {
                sftp: Channels::new(ssh.info(), viewers.clone()),
                ssh,
                viewers,
                shell_dirs: ShellDirs::default(),
                forwards,
                keep_forwards: AtomicBool::new(false),
                draining: Mutex::default(),
            });
            Self::register(&mut map, id, connection.clone(), io);
            for rule in rules_to_start(&map, &connection, &connection.ssh.profile().ssh.forwards, carry) {
                connection.forwards.start(rule, true);
            }
        }
        self.close_with(id, io);
    }

    /// Registers session `id` as another user of `connection` (a duplicated tab), until the
    /// session closes. A session closing meanwhile is unregistered right away.
    pub fn attach(&self, id: SessionId, connection: Arc<LiveConnection>, io: &TermIo) {
        Self::register(&mut self.map.lock().unwrap(), id, connection, io);
        self.close_with(id, io);
    }

    fn register(map: &mut Map, id: SessionId, connection: Arc<LiveConnection>, io: &TermIo) {
        connection.forwards.attach(id, io.sink());
        if let Some(traffic) = connection.sftp.traffic(connection.ssh.info()) {
            io.event(traffic);
        }
        connection.viewers.add(id, io.sink());
        map.insert(id, connection);
    }

    /// Unregisters session `id` when it closes. Not under the lock: a session that has
    /// already closed is unregistered at once.
    fn close_with(&self, id: SessionId, io: &TermIo) {
        let connections = self.clone();
        io.on_close(move || connections.close(id));
    }

    pub fn get(&self, id: SessionId) -> Result<Arc<LiveConnection>> {
        self.map.lock().unwrap().get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))
    }

    /// Unregisters session `id`; if no other session uses its connection, disconnects it in
    /// the background. Idempotent.
    fn close(&self, id: SessionId) {
        // The rules to carry are set aside under the lock that removes the session, so that
        // `forwards_to_carry` finds them either in the map or among those carried.
        let (connection, rules, tasks, handover) = {
            let mut connections = self.map.lock().unwrap();
            let Some(connection) = connections.remove(&id) else {
                return;
            };
            connection.forwards.detach(id);
            connection.viewers.remove(id);
            if connections.values().any(|other| Arc::ptr_eq(other, &connection)) {
                return;
            }
            let (rules, tasks) = connection.forwards.stop_all();
            let handover = connection.keep_forwards.load(Ordering::Relaxed) && !rules.is_empty();
            if !handover && !rules.is_empty() {
                let mut carried = self.carried.lock().unwrap();
                if carried.len() == CARRIED {
                    carried.pop_front();
                }
                carried.push_back((id, rules.iter().map(|rule| rule.id.clone()).collect()));
            }
            (connection, rules, tasks, handover)
        };
        let connections = self.clone();
        tauri::async_runtime::spawn(async move {
            if handover {
                // Their ports (and listeners on the server) are released first.
                for task in tasks {
                    let _ = task.await;
                }
            }
            connection.sftp.close().await;
            let _ = connection.ssh.handle().disconnect(Disconnect::ByApplication, "", "en").await;
            if handover {
                connections.adopt(&connection, rules);
            }
        });
    }

    /// Starts (or restarts with a new definition) a rule of session `id`'s saved session:
    /// where it runs, else on `id`'s connection.
    pub fn start_forward(&self, id: SessionId, rule: ForwardRule) -> Result<()> {
        let map = self.map.lock().unwrap();
        let own = map.get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))?;
        let target = session_connections(&map, &own).into_iter().find(|c| c.forwards.is_running(&rule.id)).unwrap_or(own);
        target.forwards.start(rule, false);
        Ok(())
    }

    /// Stops a rule of session `id`'s saved session wherever it runs (or clears its failure).
    pub fn stop_forward(&self, id: SessionId, rule_id: &str) -> Result<()> {
        let targets = {
            let map = self.map.lock().unwrap();
            let own = map.get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))?;
            let holders: Vec<_> = session_connections(&map, &own).into_iter().filter(|c| c.forwards.has(rule_id)).collect();
            if holders.is_empty() {
                vec![own]
            } else {
                holders
            }
        };
        for target in targets {
            target.forwards.stop(rule_id);
        }
        Ok(())
    }

    /// Stops rule `rule_id` of saved session `profile_id` wherever it runs (deleted), or all
    /// its rules (`None`: it is no longer SSH). Only those: stopping the rules a session no
    /// longer lists would also stop one another window saved and started meanwhile.
    pub fn stop_saved_forwards(&self, profile_id: &str, rule_id: Option<&str>) {
        let connections: Vec<_> = self.map.lock().unwrap().values().filter(|c| c.ssh.profile().id == profile_id).cloned().collect();
        for connection in connections {
            for rule in connection.forwards.running() {
                if rule_id.is_none_or(|id| id == rule.id) {
                    connection.forwards.stop(&rule.id);
                }
            }
        }
    }

    /// What saved session `profile_id` having changed to `profile` (`None`: deleted) does to its
    /// rules running in its tabs: one that is gone, or no longer SSH, has none, and those stop;
    /// saving an SSH session keeps them running.
    pub fn session_changed(&self, profile_id: &str, profile: Option<&Profile>) {
        if profile.is_none_or(|profile| profile.ssh().is_none()) {
            self.stop_saved_forwards(profile_id, None);
        }
    }

    /// The sessions whose connection runs forwarding rules of saved session `profile_id` (see
    /// [`forwarding_sessions`]): those deleting it would stop.
    pub fn forwarding_sessions(&self, profile_id: &str) -> Vec<SessionId> {
        forwarding_sessions(&self.map.lock().unwrap(), profile_id)
    }

    /// The rules that closing sessions `ids` would stop although another connection of their
    /// saved session stays open, which could take them over.
    pub fn forwards_to_keep(&self, ids: &[SessionId]) -> Vec<ForwardRule> {
        rules_to_keep(&self.map.lock().unwrap(), ids)
    }

    /// How many connections the rules on the connections that closing sessions `ids`
    /// disconnects carry, also those still open from before the shell last exited: those
    /// closing cuts (handing the rules over doesn't take them along).
    pub fn forward_connections(&self, ids: &[SessionId]) -> usize {
        closing(&self.map.lock().unwrap(), ids).iter().map(|connection| connection.forward_connections()).sum()
    }

    /// Moves the forwards of the connections that closing sessions `ids` disconnects to
    /// another connection of their saved session once they close. A connection another tab
    /// still uses is left alone: when it closes later, the user is asked again.
    pub fn keep_forwards(&self, ids: &[SessionId]) {
        for connection in closing(&self.map.lock().unwrap(), ids) {
            connection.keep_forwards.store(true, Ordering::Relaxed);
        }
    }

    /// The rules running on session `id`'s connection if it is the connection's last user, to
    /// carry over to the connection it reconnects with; once it has closed (the connection
    /// lost), those that were running then, given once.
    pub fn forwards_to_carry(&self, id: SessionId) -> Vec<String> {
        let map = self.map.lock().unwrap();
        if map.contains_key(&id) {
            return rules_to_carry(&map, id);
        }
        let mut carried = self.carried.lock().unwrap();
        let found = carried.iter().position(|(closed, _)| *closed == id);
        found.and_then(|at| carried.remove(at)).map(|(_, rules)| rules).unwrap_or_default()
    }

    /// Starts `rules` of a closed connection on another connection of its session, if any.
    /// Returns whether there was one.
    fn adopt(&self, closed: &LiveConnection, rules: Vec<ForwardRule>) -> bool {
        let map = self.map.lock().unwrap();
        let Some((target, rules)) = rules_to_adopt(&map, closed, rules) else { return false };
        for rule in rules {
            target.forwards.start(rule, true);
        }
        true
    }

    /// Session `id`'s shell has exited. If no other tab uses its connection, the connection's
    /// rules stop listening and move to another connection of the saved session if there is
    /// one, as the user could choose when closing the tab (exiting the shell asks nothing).
    /// Returns the connection, the connections its rules still carry, and the rules that
    /// stopped rather than moved, to start again with another shell.
    async fn release_forwards(&self, id: SessionId) -> Option<(Arc<LiveConnection>, Draining, Vec<ForwardRule>)> {
        let connection = {
            let map = self.map.lock().unwrap();
            let connection = map.get(&id)?.clone();
            if map.iter().any(|(other, c)| *other != id && Arc::ptr_eq(c, &connection)) {
                return None;
            }
            connection
        };
        let (rules, tasks) = connection.forwards.release_all();
        // Their ports (and listeners on the server) are released first.
        let mut draining = std::mem::take(&mut *connection.draining.lock().unwrap());
        draining.merge(Draining::collect(tasks).await);
        let stopped = if !rules.is_empty() && self.adopt(&connection, rules.clone()) { Vec::new() } else { rules };
        Some((connection, draining, stopped))
    }

    /// Session `id` started another shell on `connection` while its rules' connections were
    /// still open (`draining`): they go on, and the rules that stopped (`rules`) start again,
    /// except those another connection of the session runs meanwhile.
    fn restart_forwards(&self, connection: &Arc<LiveConnection>, draining: Draining, rules: Vec<ForwardRule>) {
        connection.draining.lock().unwrap().merge(draining);
        let map = self.map.lock().unwrap();
        let others = session_connections(&map, connection).split_off(1);
        for rule in rules {
            if !others.iter().any(|other| other.is_running(&rule.id)) {
                connection.forwards.start(rule, true);
            }
        }
    }
}

#[cfg(test)]
mod tests;
