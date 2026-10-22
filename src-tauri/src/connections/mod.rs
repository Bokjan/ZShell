//! The live SSH connections, keyed by the terminal sessions (tabs) that use them, with what
//! runs on each besides shells: its SFTP sessions and its forwarding rules. A duplicated tab
//! runs its shell on its source tab's connection, which stays open until the last session
//! using it closes.
//!
//! This is where the features meet: `ssh` makes the connection (and knows nothing of what else
//! runs on it), `sftp` and `forward` use it, and the registry decides where a saved session's
//! forwarding rules run and what happens to them when a connection closes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use russh::Disconnect;
use russh_sftp::client::SftpSession;

use crate::error::{Error, Result};
use crate::forward::{Forwards, Hub};
use crate::model::ForwardRule;
use crate::session::{SessionId, TermIo};
use crate::sftp::channels::Channels;
use crate::sftp::shell_dir::ShellDirs;
use crate::sftp::transfer::TransferSlots;
use crate::ssh;

/// A live SSH connection and what runs on it besides shells.
pub struct LiveConnection {
    ssh: Arc<ssh::Connection>,
    sftp: Channels,
    /// What has been found of its tabs' shells on the server.
    shell_dirs: ShellDirs,
    forwards: Forwards,
    /// Whether its forwards move to another connection of the session when it closes (the
    /// user chose so when closing its last tab).
    keep_forwards: AtomicBool,
}

impl LiveConnection {
    pub fn ssh(&self) -> &Arc<ssh::Connection> {
        &self.ssh
    }

    /// The SFTP session for browsing and editing (see [`Channels::browse`]).
    pub async fn sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
        self.sftp.browse(&self.ssh).await
    }

    /// The SFTP session for transfers (see [`Channels::transfer`]).
    pub async fn transfer_sftp(&self) -> anyhow::Result<Arc<SftpSession>> {
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
pub struct Connections(Arc<Mutex<HashMap<SessionId, Arc<LiveConnection>>>>);

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
    let some = map.values().find(|c| same_session(c.as_ref(), closed))?;
    let session = session_connections(map, some);
    let rules = rules.into_iter().filter(|rule| !session.iter().any(|c| c.is_running(&rule.id))).collect();
    Some((session[0].clone(), rules))
}

impl Connections {
    /// Registers the new connection of session `id` until the session closes, and starts the
    /// rules that start on connecting and those in `carry` (running before the tab
    /// reconnected), unless another connection of the session runs them. Forward states are
    /// reported to the session's tab.
    pub fn connected(&self, id: SessionId, ssh: Arc<ssh::Connection>, carry: &[String], io: &TermIo) {
        // Under one lock, so that two tabs of a session connecting at once share a hub, and
        // don't both start a rule (one would find its port taken).
        {
            let mut map = self.0.lock().unwrap();
            let profile_id = &ssh.profile().id;
            let sibling = map.values().find(|other| !profile_id.is_empty() && other.ssh.profile().id == *profile_id);
            let hub = sibling.map(|other| other.forwards.hub()).unwrap_or_else(|| Arc::new(Hub::default()));
            let forwards = Forwards::new(ssh.handle().clone(), ssh.routes().clone(), hub);
            let connection = Arc::new(LiveConnection {
                ssh,
                sftp: Channels::default(),
                shell_dirs: ShellDirs::default(),
                forwards,
                keep_forwards: AtomicBool::new(false),
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
        Self::register(&mut self.0.lock().unwrap(), id, connection, io);
        self.close_with(id, io);
    }

    fn register(map: &mut Map, id: SessionId, connection: Arc<LiveConnection>, io: &TermIo) {
        connection.forwards.attach(id, io.sink());
        map.insert(id, connection);
    }

    /// Unregisters session `id` when it closes. Not under the lock: a session that has
    /// already closed is unregistered at once.
    fn close_with(&self, id: SessionId, io: &TermIo) {
        let connections = self.clone();
        io.on_close(move || connections.close(id));
    }

    pub fn get(&self, id: SessionId) -> Result<Arc<LiveConnection>> {
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
        let map = self.0.lock().unwrap();
        let own = map.get(&id).cloned().ok_or_else(|| Error::new("session.notConnected"))?;
        let target = session_connections(&map, &own).into_iter().find(|c| c.forwards.is_running(&rule.id)).unwrap_or(own);
        target.forwards.start(rule, false);
        Ok(())
    }

    /// Stops a rule of session `id`'s saved session wherever it runs (or clears its failure).
    pub fn stop_forward(&self, id: SessionId, rule_id: &str) -> Result<()> {
        let targets = {
            let map = self.0.lock().unwrap();
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
        let connections: Vec<_> = self.0.lock().unwrap().values().filter(|c| c.ssh.profile().id == profile_id).cloned().collect();
        for connection in connections {
            for rule in connection.forwards.running() {
                if rule_id.is_none_or(|id| id == rule.id) {
                    connection.forwards.stop(&rule.id);
                }
            }
        }
    }

    /// The sessions whose connection runs forwarding rules of saved session `profile_id` (see
    /// [`forwarding_sessions`]): those deleting it would stop.
    pub fn forwarding_sessions(&self, profile_id: &str) -> Vec<SessionId> {
        forwarding_sessions(&self.0.lock().unwrap(), profile_id)
    }

    /// The rules that closing sessions `ids` would stop although another connection of their
    /// saved session stays open, which could take them over.
    pub fn forwards_to_keep(&self, ids: &[SessionId]) -> Vec<ForwardRule> {
        rules_to_keep(&self.0.lock().unwrap(), ids)
    }

    /// Moves the forwards of the connections that closing sessions `ids` disconnects to
    /// another connection of their saved session once they close. A connection another tab
    /// still uses is left alone: when it closes later, the user is asked again.
    pub fn keep_forwards(&self, ids: &[SessionId]) {
        for connection in closing(&self.0.lock().unwrap(), ids) {
            connection.keep_forwards.store(true, Ordering::Relaxed);
        }
    }

    /// The rules running on session `id`'s connection if it is the connection's last user, to
    /// carry over to the connection it reconnects with.
    pub fn forwards_to_carry(&self, id: SessionId) -> Vec<String> {
        rules_to_carry(&self.0.lock().unwrap(), id)
    }

    /// Starts `rules` of a closed connection on another connection of its session, if any.
    fn adopt(&self, closed: &LiveConnection, rules: Vec<ForwardRule>) {
        let map = self.0.lock().unwrap();
        let Some((target, rules)) = rules_to_adopt(&map, closed, rules) else { return };
        for rule in rules {
            target.forwards.start(rule, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forward::testing::{free_port, local_rule, until, Tab};
    use crate::model::ForwardKind;
    use crate::session::Outcome;
    use crate::ssh::testing::{connect, Anyone};

    /// Two connections of saved session "web", by tabs 1 and 2, with rule r1 running on the
    /// first; tab 2 watches the rule.
    async fn two_connections(port: u16) -> (Connections, [Arc<ssh::Connection>; 2], [Tab; 2]) {
        let connections = Connections::default();
        let ssh = [connect(Anyone, "web").await, connect(Anyone, "web").await];
        let tabs = [Tab::new(), Tab::new()];
        connections.connected(1, ssh[0].clone(), &[], &tabs[0].io);
        connections.connected(2, ssh[1].clone(), &[], &tabs[1].io);
        connections.start_forward(1, local_rule("r1", port)).unwrap();
        assert_eq!(tabs[1].next_kind("r1").await, "starting");
        assert_eq!(tabs[1].next_kind("r1").await, "active");
        assert!(connections.get(1).unwrap().forwards.is_running("r1"));
        (connections, ssh, tabs)
    }

    /// Closing the connection running a rule the user chose to keep: the rule moves to the
    /// session's other connection, on the same port, once the first has let it go.
    #[tokio::test]
    async fn hands_rules_over_when_a_connection_closes() {
        let port = free_port();
        let (connections, ssh, tabs) = two_connections(port).await;
        assert_eq!(ids(&connections.forwards_to_keep(&[1])), ["r1"]);
        connections.keep_forwards(&[1]);
        let closing = connections.get(1).unwrap();
        tabs[0].io.finish(Outcome::Exited(None));

        assert_eq!(tabs[1].next_kind("r1").await, "stopped");
        assert_eq!(tabs[1].next_kind("r1").await, "starting");
        let active = tabs[1].next_state("r1").await;
        assert_eq!(active["type"], "active", "{active}");
        assert_eq!(active["bound"], format!("127.0.0.1:{port}"));
        until("the closing task's end", || Arc::strong_count(&closing) == 1).await;
        until("the connection's end", || ssh[0].handle().is_closed()).await;
        assert!(connections.get(1).is_err());
        assert!(connections.get(2).unwrap().forwards.is_running("r1"));
    }

    /// The tabs using a connection that runs the session's rules, whichever tab started them.
    #[tokio::test]
    async fn finds_the_tabs_forwarding_for_a_session() {
        let (connections, ssh, tabs) = two_connections(0).await;
        let duplicate = Tab::new();
        connections.attach(3, connections.get(1).unwrap(), &duplicate.io);
        assert_eq!(connections.forwarding_sessions("web"), [1, 3]);
        assert!(connections.forwarding_sessions("other").is_empty());
        connections.stop_saved_forwards("web", None);
        assert_eq!(tabs[1].next_kind("r1").await, "stopped");
        assert!(connections.forwarding_sessions("web").is_empty());
        drop(ssh);
    }

    /// Without the user choosing to keep them, a closing connection's rules just stop.
    #[tokio::test]
    async fn stops_rules_not_kept_when_a_connection_closes() {
        let (connections, ssh, tabs) = two_connections(0).await;
        let closing = connections.get(1).unwrap();
        tabs[0].io.finish(Outcome::Exited(None));
        assert_eq!(tabs[1].next_kind("r1").await, "stopped");
        // The task that disconnects it, and would hand the rules over, has let it go.
        until("the closing task's end", || Arc::strong_count(&closing) == 1).await;
        until("the connection's end", || ssh[0].handle().is_closed()).await;
        assert!(!connections.get(2).unwrap().forwards.has("r1"));
        assert_eq!(tabs[1].reported("r1"), Vec::<String>::new());
    }

    /// A connection another tab still uses (a duplicated tab) stays open with its rules.
    #[tokio::test]
    async fn keeps_rules_while_another_tab_uses_the_connection() {
        let (connections, ssh, tabs) = two_connections(0).await;
        let duplicate = Tab::new();
        connections.attach(3, connections.get(1).unwrap(), &duplicate.io);
        tabs[0].io.finish(Outcome::Exited(None));
        assert!(connections.get(1).is_err());
        assert!(connections.get(3).unwrap().forwards.is_running("r1"));
        assert!(!ssh[0].handle().is_closed());
        assert!(tabs[1].quiet());
    }

    /// A connection of saved session `session` running `running`.
    struct Fake {
        session: &'static str,
        running: Vec<ForwardRule>,
    }

    impl Holder for Fake {
        fn session(&self) -> &str {
            self.session
        }
        fn running(&self) -> Vec<ForwardRule> {
            self.running.clone()
        }
        fn is_running(&self, rule_id: &str) -> bool {
            self.running.iter().any(|rule| rule.id == rule_id)
        }
    }

    fn rule(id: &str, auto_start: bool) -> ForwardRule {
        ForwardRule {
            id: id.into(),
            kind: ForwardKind::Local,
            bind_host: "localhost".into(),
            bind_port: 8080,
            target_host: "localhost".into(),
            target_port: 80,
            description: String::new(),
            auto_start,
        }
    }

    fn connection(session: &'static str, running: &[&str]) -> Arc<Fake> {
        Arc::new(Fake { session, running: running.iter().map(|id| rule(id, false)).collect() })
    }

    fn ids(rules: &[ForwardRule]) -> Vec<&str> {
        let mut ids: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
        ids.sort();
        ids
    }

    /// Tab 1 and its duplicate (tab 2) share a connection running r1; tab 3 is another
    /// connection of the same session; tab 4 one of another session.
    fn tabs() -> (Map<Fake>, Arc<Fake>, Arc<Fake>) {
        let shared = connection("web", &["r1"]);
        let other = connection("web", &[]);
        let map = HashMap::from([(1, shared.clone()), (2, shared.clone()), (3, other.clone()), (4, connection("db", &["r9"]))]);
        (map, shared, other)
    }

    #[test]
    fn starts_a_session_s_rules_once() {
        let (map, shared, other) = tabs();
        let rules = [rule("r1", true), rule("r2", true), rule("r3", false), rule("r4", false)];
        // r1 runs on the session's other connection; r4 ran before reconnecting.
        assert_eq!(ids(&rules_to_start(&map, &other, &rules, &["r4".into()])), ["r2", "r4"]);
        assert_eq!(ids(&rules_to_start(&map, &shared, &rules, &[])), ["r1", "r2"]);
        // A quick connection shares rules with nothing.
        let quick = connection("", &[]);
        assert_eq!(
            ids(&rules_to_start(&HashMap::from([(5, quick.clone()), (6, connection("", &["r1"]))]), &quick, &rules, &[])),
            ["r1", "r2"]
        );
    }

    #[test]
    fn offers_to_keep_rules_another_connection_could_take_over() {
        let (map, _, _) = tabs();
        // Closing one of the shared connection's tabs closes nothing.
        assert!(rules_to_keep(&map, &[1]).is_empty());
        // Closing both closes it, and tab 3 stays.
        assert_eq!(ids(&rules_to_keep(&map, &[1, 2])), ["r1"]);
        // Closing every tab of the session leaves nothing to take them over.
        assert!(rules_to_keep(&map, &[1, 2, 3]).is_empty());
        // Another session's tab staying open doesn't count.
        assert!(rules_to_keep(&HashMap::from([(1, connection("web", &["r1"])), (4, connection("db", &[]))]), &[1]).is_empty());
    }

    #[test]
    fn finds_the_connections_that_close() {
        let (map, shared, other) = tabs();
        // Tab 2 still uses the shared connection.
        assert!(closing(&map, &[1, 3]).iter().all(|c| Arc::ptr_eq(c, &other)));
        assert_eq!(closing(&map, &[1, 3]).len(), 1);
        let both = closing(&map, &[1, 2]);
        assert_eq!(both.len(), 1);
        assert!(Arc::ptr_eq(&both[0], &shared));
    }

    #[test]
    fn carries_rules_over_a_reconnection_by_the_last_user() {
        let (mut map, _, _) = tabs();
        // Tab 2 still uses the connection: tab 1 reconnecting takes nothing with it.
        assert!(rules_to_carry(&map, 1).is_empty());
        map.remove(&2);
        assert_eq!(rules_to_carry(&map, 1), ["r1"]);
        assert!(rules_to_carry(&map, 7).is_empty());
    }

    #[test]
    fn hands_rules_over_to_another_connection_of_the_session() {
        let (mut map, shared, other) = tabs();
        map.remove(&1);
        map.remove(&2);
        let (target, rules) = rules_to_adopt(&map, &shared, vec![rule("r1", false)]).unwrap();
        assert!(Arc::ptr_eq(&target, &other));
        assert_eq!(ids(&rules), ["r1"]);
        // None left of the session.
        map.remove(&3);
        assert!(rules_to_adopt(&map, &shared, vec![rule("r1", false)]).is_none());
    }
}
