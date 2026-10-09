//! Port forwarding over a session's SSH connection: local (`-L`), remote (`-R`) and dynamic
//! SOCKS (`-D`).
//!
//! Each running rule is one task that owns its listener (or, for remote rules, its route in
//! [`RemoteRoutes`]) and a [`JoinSet`] of the connections it carries, so stopping a rule
//! closes everything it opened. Rules belong to a saved session, which may have several
//! connections (tabs opened separately): state changes are pushed as [`SessionEvent::Forward`]
//! to every tab of the session through its [`Hub`], whichever connection runs the rule (see
//! `ssh::Connections` for how a rule runs on one connection only). A generation number keeps a
//! replaced or stopped task from reporting after the fact.

mod dynamic;
mod local;
mod remote;
pub(crate) mod socks;

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::Msg;
use russh::Channel;
use serde::{Deserialize, Serialize};
use tauri::async_runtime::JoinHandle;
use tokio::net::TcpStream;
use tokio::task::{JoinError, JoinSet};

use crate::error::{Error, Result};
use crate::session::{SessionEvent, SessionId, SessionSink};
use crate::ssh::SshHandle;
pub use remote::{Incoming, RemoteRoutes};

/// How long to wait for a TCP connection to a forward's target on this computer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardKind {
    /// `-L`: listen here, connect from the server.
    Local,
    /// `-R`: listen on the server, connect from here.
    Remote,
    /// `-D`: SOCKS proxy here, connect from the server.
    Dynamic,
}

/// A saved forwarding rule. `target_*` is unused for dynamic rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardRule {
    /// Empty for a new rule; assigned on save.
    #[serde(default)]
    pub id: String,
    pub kind: ForwardKind,
    #[serde(default)]
    pub bind_host: String,
    /// 0 lets the OS (or the server, for remote rules) pick a port.
    pub bind_port: u16,
    #[serde(default)]
    pub target_host: String,
    #[serde(default)]
    pub target_port: u16,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub auto_start: bool,
}

impl ForwardRule {
    /// Trims fields, fills in the default bind address and checks the target.
    pub fn normalize(mut self) -> Result<Self> {
        self.bind_host = self.bind_host.trim().to_owned();
        self.target_host = self.target_host.trim().to_owned();
        self.description = self.description.trim().to_owned();
        if self.bind_host.is_empty() {
            // Loopback by default, as OpenSSH does; the server resolves "localhost" itself.
            self.bind_host = if self.kind == ForwardKind::Remote { "localhost" } else { "127.0.0.1" }.to_owned();
        }
        if self.kind == ForwardKind::Dynamic {
            self.target_host.clear();
            self.target_port = 0;
        } else if self.target_host.is_empty() || self.target_port == 0 {
            return Err(Error::new("forward.missingTarget"));
        }
        Ok(self)
    }
}

/// Formats `host:port`, bracketing IPv6 literals.
pub fn host_port(host: &str, port: impl fmt::Display) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ForwardState {
    Starting,
    Active {
        /// The address actually listened on (with the real port when 0 was requested).
        bound: String,
        connections: usize,
        /// Why the most recent connection through this rule failed, if one did.
        last_error: Option<Error>,
    },
    Failed { error: Error },
    Stopped,
}

struct Running {
    rule: ForwardRule,
    /// `None` once stopped; the task is kept so a restart can wait for it to wind down.
    generation: Option<u64>,
    task: JoinHandle<()>,
}

/// The tabs of one saved session (on any of its connections), and the latest state of each
/// of its rules, so a tab that attaches later starts with the current states.
#[derive(Default)]
pub struct Hub(Mutex<Watchers>);

#[derive(Default)]
struct Watchers {
    sinks: Vec<(SessionId, SessionSink)>,
    states: HashMap<String, ForwardState>,
}

impl Hub {
    fn report(&self, rule_id: &str, state: ForwardState) {
        let mut watchers = self.0.lock().unwrap();
        for (_, sink) in &watchers.sinks {
            sink.event(SessionEvent::Forward { rule_id: rule_id.to_owned(), state: state.clone() });
        }
        watchers.states.insert(rule_id.to_owned(), state);
    }

    fn print(&self, text: &str) {
        for (_, sink) in &self.0.lock().unwrap().sinks {
            sink.print(text);
        }
    }
}

struct Shared {
    handle: Arc<SshHandle>,
    routes: RemoteRoutes,
    hub: Arc<Hub>,
    running: Mutex<HashMap<String, Running>>,
    /// In-flight `cancel-tcpip-forward` requests of stopped remote rules, by rule id.
    cancels: Mutex<HashMap<String, JoinHandle<()>>>,
    next_generation: AtomicU64,
}

/// The forwards running on one SSH connection.
pub struct Forwards(Arc<Shared>);

impl Forwards {
    /// `hub`: the session's, shared with its other connections.
    pub fn new(handle: Arc<SshHandle>, routes: RemoteRoutes, hub: Arc<Hub>) -> Self {
        Self(Arc::new(Shared {
            handle,
            routes,
            hub,
            running: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
        }))
    }

    pub fn hub(&self) -> Arc<Hub> {
        self.0.hub.clone()
    }

    /// Reports rule states to session `id` from now on, starting with the current ones.
    pub fn attach(&self, id: SessionId, sink: SessionSink) {
        let mut watchers = self.0.hub.0.lock().unwrap();
        for (rule_id, state) in &watchers.states {
            sink.event(SessionEvent::Forward { rule_id: rule_id.clone(), state: state.clone() });
        }
        watchers.sinks.push((id, sink));
    }

    pub fn detach(&self, id: SessionId) {
        self.0.hub.0.lock().unwrap().sinks.retain(|(watcher, _)| *watcher != id);
    }

    /// Whether `rule_id` runs here (or is starting): not stopped, and not ended by a failure.
    pub fn is_running(&self, rule_id: &str) -> bool {
        self.0.running.lock().unwrap().get(rule_id).is_some_and(|r| r.generation.is_some() && !r.task.inner().is_finished())
    }

    /// Whether `rule_id` was started here and not stopped since (it may have failed).
    pub fn has(&self, rule_id: &str) -> bool {
        self.0.running.lock().unwrap().get(rule_id).is_some_and(|r| r.generation.is_some())
    }

    /// The rules running here.
    pub fn running(&self) -> Vec<ForwardRule> {
        let running = self.0.running.lock().unwrap();
        running.values().filter(|r| r.generation.is_some() && !r.task.inner().is_finished()).map(|r| r.rule.clone()).collect()
    }

    /// Starts `rule`, restarting it if it is already running. With `announce`, a failure to
    /// start is also printed in the terminal (for rules started automatically on connect).
    pub fn start(&self, rule: ForwardRule, announce: bool) {
        let generation = self.0.next_generation.fetch_add(1, Ordering::Relaxed);
        let ctx = Ctx { shared: self.0.clone(), rule_id: rule.id.clone(), generation };
        // Hold the lock until the new task is registered, so its first report is not dropped.
        let mut running = self.0.running.lock().unwrap();
        let previous = running.remove(&rule.id).map(|old| {
            old.task.abort();
            old.task
        });
        self.0.report(&rule.id, ForwardState::Starting);
        let task = tauri::async_runtime::spawn(run(rule.clone(), ctx, previous, announce));
        running.insert(rule.id.clone(), Running { rule, generation: Some(generation), task });
    }

    pub fn stop(&self, rule_id: &str) {
        let mut running = self.0.running.lock().unwrap();
        if let Some(old) = running.get_mut(rule_id) {
            old.task.abort();
            old.generation = None;
        }
        self.0.report(rule_id, ForwardState::Stopped);
    }

    /// Stops every rule, as the connection closes; the session's other tabs see them stop.
    /// Returns the rules that were running, and their tasks, which end shortly.
    pub fn stop_all(&self) -> (Vec<ForwardRule>, Vec<JoinHandle<()>>) {
        let mut rules = Vec::new();
        let mut tasks = Vec::new();
        for (rule_id, old) in self.0.running.lock().unwrap().drain() {
            if old.generation.is_some() && !old.task.inner().is_finished() {
                self.0.report(&rule_id, ForwardState::Stopped);
                rules.push(old.rule);
            }
            old.task.abort();
            tasks.push(old.task);
        }
        (rules, tasks)
    }
}

impl Shared {
    fn report(&self, rule_id: &str, state: ForwardState) {
        self.hub.report(rule_id, state);
    }

    fn print(&self, text: &str) {
        self.hub.print(text);
    }
}

/// What a rule's task needs: the connection, and a way to report its own state.
#[derive(Clone)]
struct Ctx {
    shared: Arc<Shared>,
    rule_id: String,
    generation: u64,
}

impl Ctx {
    fn handle(&self) -> Arc<SshHandle> {
        self.shared.handle.clone()
    }

    fn report(&self, state: ForwardState) {
        // Checked under the lock that `start` / `stop` hold while replacing a task, so a
        // superseded task can never overwrite the state of its successor.
        let running = self.shared.running.lock().unwrap();
        if running.get(&self.rule_id).is_some_and(|r| r.generation == Some(self.generation)) {
            self.shared.report(&self.rule_id, state);
        }
    }
}

async fn run(rule: ForwardRule, ctx: Ctx, previous: Option<JoinHandle<()>>, announce: bool) {
    // Let the previous instance release its port (and, for remote rules, the server's
    // listener) before binding it again.
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    let cancel = ctx.shared.cancels.lock().unwrap().remove(&rule.id);
    if let Some(cancel) = cancel {
        let _ = cancel.await;
    }

    let result = match rule.kind {
        ForwardKind::Local => local::run(&rule, &ctx).await,
        ForwardKind::Remote => remote::run(&rule, &ctx).await,
        ForwardKind::Dynamic => dynamic::run(&rule, &ctx).await,
    };
    if let Err(e) = result {
        let error = Error::from(e);
        if announce {
            let forward = describe(&rule);
            let message = t!("forward.autoStartFailed", forward = forward, error = error);
            ctx.shared.print(&format!("\r\n\x1b[33m{message}\x1b[0m\n"));
        }
        ctx.report(ForwardState::Failed { error });
    }
}

/// OpenSSH-style one-line summary of a rule, e.g. `-L 127.0.0.1:8080:db:5432`.
fn describe(rule: &ForwardRule) -> String {
    let bind = host_port(&rule.bind_host, rule.bind_port);
    match rule.kind {
        ForwardKind::Local => format!("-L {bind}:{}", host_port(&rule.target_host, rule.target_port)),
        ForwardKind::Remote => format!("-R {bind}:{}", host_port(&rule.target_host, rule.target_port)),
        ForwardKind::Dynamic => format!("-D {bind}"),
    }
}

/// The connections carried by a running rule, reported as its `Active` state.
struct Tracker<'a> {
    ctx: &'a Ctx,
    bound: String,
    connections: JoinSet<anyhow::Result<()>>,
    last_error: Option<Error>,
}

impl<'a> Tracker<'a> {
    fn new(ctx: &'a Ctx, bound: String) -> Self {
        let tracker = Self { ctx, bound, connections: JoinSet::new(), last_error: None };
        tracker.report();
        tracker
    }

    fn spawn(&mut self, connection: impl Future<Output = anyhow::Result<()>> + Send + 'static) {
        self.connections.spawn(connection);
        self.report();
    }

    /// Waits for a connection to end; pending forever while there are none, for `select!`.
    async fn next_finished(&mut self) -> std::result::Result<anyhow::Result<()>, JoinError> {
        match self.connections.join_next().await {
            Some(result) => result,
            None => std::future::pending().await,
        }
    }

    fn finished(&mut self, result: std::result::Result<anyhow::Result<()>, JoinError>) {
        if let Ok(Err(e)) = result {
            self.last_error = Some(e.into());
        }
        self.report();
    }

    fn failed(&mut self, error: impl Into<Error>) {
        self.last_error = Some(error.into());
        self.report();
    }

    fn report(&self) {
        self.ctx.report(ForwardState::Active {
            bound: self.bound.clone(),
            connections: self.connections.len(),
            last_error: self.last_error.clone(),
        });
    }
}

/// Copies data both ways until both sides are done. Errors here are ordinary disconnects
/// (resets, aborted transfers) and are not reported.
async fn bridge(mut stream: TcpStream, channel: Channel<Msg>) {
    let _ = stream.set_nodelay(true);
    let mut channel = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel).await;
}
