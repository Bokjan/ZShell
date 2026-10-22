//! Port forwarding over a session's SSH connection: local (`-L`), remote (`-R`) and dynamic
//! SOCKS (`-D`).
//!
//! Each run of a rule is one task that owns everything the rule holds: its listener (for
//! remote rules, its route in the connection's `ssh::RemoteRoutes` and the server's listener) and a [`JoinSet`] of
//! the connections it carries. Stopping a rule signals the task, which releases all of it (a
//! remote rule waits for the server to cancel its listener) and then ends: once the task has
//! ended, the rule can start again on the same port. Dropping the signal stops it too. Rules
//! belong to a saved session, which may have several connections (tabs opened separately):
//! state changes are pushed as [`SessionEvent::Forward`] to every tab of the session through
//! its [`Hub`], whichever connection runs the rule (see `crate::connections` for how a rule
//! runs on one connection only). A generation number keeps a replaced or stopped task from
//! reporting after the fact.

mod dynamic;
mod local;
mod remote;
pub(crate) mod socks;
#[cfg(test)]
pub(crate) mod testing;

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::Msg;
use russh::Channel;
use tauri::async_runtime::JoinHandle;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet};
use tokio::time::Instant;

use crate::error::Error;
use crate::model::{host_port, ForwardKind, ForwardRule};
use crate::session::{ForwardState, SessionEvent, SessionId, SessionSink};
use crate::ssh::{Liveness, RemoteRoutes, SshHandle};

/// How long to wait for the name a local or dynamic rule listens on to be looked up.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);

/// How long the server may take to open a rule's channel. It answers once it has connected to
/// the target, and one that drops packets would otherwise leave the client waiting until the
/// server's kernel gives up (two minutes on Linux), with nothing to show in the rule's state.
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);

struct Running {
    rule: ForwardRule,
    /// `None` once stopped; the task is kept so a restart can wait for it to wind down.
    generation: Option<u64>,
    /// Stops the task, also when dropped.
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// A rule task's side of its stop signal.
pub(super) struct Stopping(watch::Receiver<bool>);

impl Stopping {
    /// Completes once the rule is asked to stop (or its owner is gone).
    async fn requested(&mut self) {
        let _ = self.0.wait_for(|stop| *stop).await;
    }
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
    liveness: Liveness,
    hub: Arc<Hub>,
    running: Mutex<HashMap<String, Running>>,
    next_generation: AtomicU64,
    /// The connection is closing (`stop_all`): nothing starts any more. Set and read under
    /// the `running` lock.
    closed: AtomicBool,
}

/// The forwards running on one SSH connection.
pub struct Forwards(Arc<Shared>);

impl Forwards {
    /// `hub`: the session's, shared with its other connections.
    pub fn new(connection: &crate::ssh::Connection, hub: Arc<Hub>) -> Self {
        Self(Arc::new(Shared {
            handle: connection.handle().clone(),
            routes: connection.routes().clone(),
            liveness: connection.liveness().clone(),
            hub,
            running: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
            closed: AtomicBool::new(false),
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
        // Asked for on a connection that is closing meanwhile (from another tab).
        if self.0.closed.load(Ordering::Relaxed) {
            self.0.report(&rule.id, ForwardState::Stopped);
            return;
        }
        // Dropping it stops the previous run.
        let previous = running.remove(&rule.id).map(|old| old.task);
        self.0.report(&rule.id, ForwardState::Starting);
        let (stop, stopping) = watch::channel(false);
        let task = tauri::async_runtime::spawn(run(rule.clone(), ctx, previous, announce, Stopping(stopping)));
        running.insert(rule.id.clone(), Running { rule, generation: Some(generation), stop, task });
    }

    pub fn stop(&self, rule_id: &str) {
        let mut running = self.0.running.lock().unwrap();
        if let Some(old) = running.get_mut(rule_id) {
            let _ = old.stop.send(true);
            old.generation = None;
        }
        self.0.report(rule_id, ForwardState::Stopped);
    }

    /// Stops every rule, as the connection closes; the session's other tabs see them stop.
    /// Returns the rules that were running, and their tasks, which end once they have
    /// released their ports and the server's listeners.
    pub fn stop_all(&self) -> (Vec<ForwardRule>, Vec<JoinHandle<()>>) {
        let mut rules = Vec::new();
        let mut tasks = Vec::new();
        let mut running = self.0.running.lock().unwrap();
        self.0.closed.store(true, Ordering::Relaxed);
        for (rule_id, old) in running.drain() {
            if old.generation.is_some() && !old.task.inner().is_finished() {
                self.0.report(&rule_id, ForwardState::Stopped);
                rules.push(old.rule);
            }
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

    /// What opens the channels of the connections a local or dynamic rule takes.
    fn opener(&self) -> Opener {
        Opener { handle: self.shared.handle.clone(), liveness: self.shared.liveness.clone() }
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

async fn run(rule: ForwardRule, ctx: Ctx, previous: Option<JoinHandle<()>>, announce: bool, mut stopping: Stopping) {
    // Let the previous run release its port (and, for remote rules, the server's listener)
    // before binding it again.
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    // Ends with `Ok` once stopped.
    let result = match rule.kind {
        ForwardKind::Local => local::run(&rule, &ctx, &mut stopping).await,
        ForwardKind::Remote => remote::run(&rule, &ctx, &mut stopping).await,
        ForwardKind::Dynamic => dynamic::run(&rule, &ctx, &mut stopping).await,
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

/// The least time between two reports of a rule's connections: a browser using a dynamic
/// rule opens and closes hundreds a second, and each report goes to every tab of the session.
const REPORT_INTERVAL: Duration = Duration::from_millis(200);

/// When to report changes that come faster than [`REPORT_INTERVAL`]: the first at once, the
/// rest together once the interval has passed.
#[derive(Default)]
struct Throttle {
    reported: Option<Instant>,
    /// A change not reported yet.
    pending: bool,
}

impl Throttle {
    /// Whether to report a change now; otherwise it is reported when [`Throttle::due`] says.
    fn changed(&mut self, now: Instant) -> bool {
        if self.reported.is_some_and(|reported| now < reported + REPORT_INTERVAL) {
            self.pending = true;
            return false;
        }
        self.reported(now);
        true
    }

    fn reported(&mut self, now: Instant) {
        self.reported = Some(now);
        self.pending = false;
    }

    /// When the change put off is due, if there is one.
    fn due(&self) -> Option<Instant> {
        self.pending.then(|| self.reported.map_or_else(Instant::now, |reported| reported + REPORT_INTERVAL))
    }
}

/// Completes at `deadline`; pending forever without one, for `select!`.
async fn at(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// The connections carried by a running rule, reported as its `Active` state.
struct Tracker<'a> {
    ctx: &'a Ctx,
    bound: String,
    connections: JoinSet<anyhow::Result<()>>,
    last_error: Option<Error>,
    throttle: Throttle,
}

impl<'a> Tracker<'a> {
    fn new(ctx: &'a Ctx, bound: String) -> Self {
        let mut tracker = Self { ctx, bound, connections: JoinSet::new(), last_error: None, throttle: Throttle::default() };
        tracker.changed();
        tracker
    }

    fn spawn(&mut self, connection: impl Future<Output = anyhow::Result<()>> + Send + 'static) {
        self.connections.spawn(connection);
        self.changed();
    }

    /// When changes put off by [`REPORT_INTERVAL`] are to be reported (with [`Tracker::report`]).
    fn report_due(&self) -> Option<Instant> {
        self.throttle.due()
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
        self.changed();
    }

    fn failed(&mut self, error: impl Into<Error>) {
        self.last_error = Some(error.into());
        self.changed();
    }

    fn changed(&mut self) {
        if self.throttle.changed(Instant::now()) {
            self.send();
        }
    }

    /// Reports what changed since the last report.
    fn report(&mut self) {
        self.throttle.reported(Instant::now());
        self.send();
    }

    fn send(&self) {
        self.ctx.report(ForwardState::Active {
            bound: self.bound.clone(),
            connections: self.connections.len(),
            last_error: self.last_error.clone(),
        });
    }
}

/// Opens the `direct-tcpip` channels of a rule's connections.
#[derive(Clone)]
struct Opener {
    handle: Arc<SshHandle>,
    liveness: Liveness,
}

impl Opener {
    /// A channel to `host:port` for the connection from `peer`, within [`OPEN_TIMEOUT`]. The
    /// server answers the request, which tells that the connection is still alive.
    async fn open(&self, host: String, port: u16, peer: SocketAddr) -> anyhow::Result<Channel<Msg>> {
        self.liveness.expect_reply();
        let opening = self.handle.channel_open_direct_tcpip(host, port.into(), peer.ip().to_string(), peer.port().into());
        Ok(tokio::time::timeout(OPEN_TIMEOUT, opening).await??)
    }
}

/// Copies data both ways until both sides are done. Errors here are ordinary disconnects
/// (resets, aborted transfers) and are not reported. Each write to the channel is a packet
/// of its own: buffers larger than the default 8 KiB send fewer when there is much to copy.
async fn bridge(mut stream: TcpStream, channel: Channel<Msg>) {
    const BUFFER: usize = 64 << 10;
    let _ = stream.set_nodelay(true);
    let mut channel = channel.into_stream();
    let _ = tokio::io::copy_bidirectional_with_sizes(&mut stream, &mut channel, BUFFER, BUFFER).await;
}

#[cfg(test)]
mod tests {
    use super::testing::{free_port, local_rule, until, Tab};
    use super::*;
    use crate::ssh::testing::{connect, Anyone};

    /// The forwards of a new connection, and a tab of its session watching them.
    async fn forwards() -> (Forwards, Tab) {
        let connection = connect(Anyone, "web").await;
        let forwards = Forwards::new(&connection, Arc::new(Hub::default()));
        let tab = Tab::new();
        forwards.attach(1, tab.io.sink());
        (forwards, tab)
    }

    /// Whether the task of rule `rule_id` has ended.
    fn ended(forwards: &Forwards, rule_id: &str) -> bool {
        forwards.0.running.lock().unwrap().get(rule_id).is_some_and(|r| r.task.inner().is_finished())
    }

    #[tokio::test]
    async fn starts_and_stops_a_rule() {
        let (forwards, tab) = forwards().await;
        forwards.start(local_rule("r1", 0), false);
        assert_eq!(tab.next_kind("r1").await, "starting");
        let active = tab.next_state("r1").await;
        assert_eq!(active["type"], "active");
        assert!(active["bound"].as_str().unwrap().starts_with("127.0.0.1:"));
        assert!(forwards.has("r1") && forwards.is_running("r1"));
        assert_eq!(forwards.running().len(), 1);

        forwards.stop("r1");
        assert_eq!(tab.next_kind("r1").await, "stopped");
        assert!(!forwards.has("r1") && !forwards.is_running("r1"));
        assert!(forwards.running().is_empty());
    }

    /// A rule started again on the same port: the new run binds it once the old one has let
    /// it go, and the old one's reports no longer count.
    #[tokio::test]
    async fn restarting_a_rule_reuses_its_port() {
        let (forwards, tab) = forwards().await;
        let rule = local_rule("r1", free_port());
        forwards.start(rule.clone(), false);
        assert_eq!(tab.next_kind("r1").await, "starting");
        let first = tab.next_state("r1").await;
        assert_eq!(first["type"], "active");

        forwards.start(rule, false);
        assert_eq!(tab.next_kind("r1").await, "starting");
        let second = tab.next_state("r1").await;
        assert_eq!(second["type"], "active", "{second}");
        assert_eq!(second["bound"], first["bound"]);
        assert_eq!(forwards.running().len(), 1);

        // The replaced run's generation reports nothing; the current one's does.
        let ctx = |generation| Ctx { shared: forwards.0.clone(), rule_id: "r1".into(), generation };
        let current = forwards.0.next_generation.load(Ordering::Relaxed) - 1;
        assert!(tab.quiet());
        ctx(current - 1).report(ForwardState::Stopped);
        assert!(tab.quiet());
        ctx(current).report(ForwardState::Stopped);
        assert_eq!(tab.next_kind("r1").await, "stopped");
    }

    /// Each start takes a new generation, and a stopped rule's run reports nothing more.
    #[tokio::test]
    async fn a_stopped_run_no_longer_reports() {
        let (forwards, tab) = forwards().await;
        forwards.start(local_rule("r1", 0), false);
        forwards.start(local_rule("r2", 0), false);
        assert_eq!(forwards.0.next_generation.load(Ordering::Relaxed), 2);
        assert_eq!(tab.next_kind("r1").await, "starting");
        assert_eq!(tab.next_kind("r1").await, "active");

        forwards.stop("r1");
        assert_eq!(tab.next_kind("r1").await, "stopped");
        Ctx { shared: forwards.0.clone(), rule_id: "r1".into(), generation: 0 }.report(ForwardState::Starting);
        until("the stopped run's end", || ended(&forwards, "r1")).await;
        // Its run has ended: nothing more can come.
        assert_eq!(tab.reported("r1"), Vec::<String>::new());
        assert!(forwards.running().iter().all(|rule| rule.id == "r2"));
    }

    /// Once the connection closes, its rules stop and none starts again.
    #[tokio::test]
    async fn nothing_starts_after_stop_all() {
        let (forwards, tab) = forwards().await;
        forwards.start(local_rule("r1", 0), false);
        assert_eq!(tab.next_kind("r1").await, "starting");
        assert_eq!(tab.next_kind("r1").await, "active");

        let (rules, tasks) = forwards.stop_all();
        assert_eq!(rules.iter().map(|rule| rule.id.as_str()).collect::<Vec<_>>(), ["r1"]);
        assert_eq!(tab.next_kind("r1").await, "stopped");
        for task in tasks {
            task.await.unwrap();
        }

        forwards.start(local_rule("r1", 0), false);
        assert_eq!(tab.next_kind("r1").await, "stopped");
        assert!(tab.quiet());
        assert!(!forwards.has("r1") && forwards.running().is_empty());
        assert!(forwards.0.running.lock().unwrap().is_empty());
    }

    /// A rule that failed to start is still had (until stopped), but no longer runs.
    #[tokio::test]
    async fn a_failed_rule_is_not_running() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let (forwards, tab) = forwards().await;
        forwards.start(local_rule("r1", port), false);
        assert_eq!(tab.next_kind("r1").await, "starting");
        let failed = tab.next_state("r1").await;
        assert_eq!(failed["type"], "failed", "{failed}");
        until("the failed run's end", || ended(&forwards, "r1")).await;
        assert!(forwards.has("r1"));
        assert!(!forwards.is_running("r1"));
        assert!(forwards.running().is_empty());
    }

    /// A server that never answers requests for `direct-tcpip` channels, as one waiting to
    /// connect to a target that drops packets.
    struct Unanswering(Vec<russh::server::ChannelOpenHandle>);

    impl russh::server::Handler for Unanswering {
        type Error = russh::Error;

        async fn auth_none(&mut self, _user: &str) -> Result<russh::server::Auth, Self::Error> {
            Ok(russh::server::Auth::Accept)
        }

        async fn channel_open_direct_tcpip(
            &mut self,
            _channel: Channel<russh::server::Msg>,
            _host: &str,
            _port: u32,
            _originator: &str,
            _originator_port: u32,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            self.0.push(reply);
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_on_a_channel_the_server_never_opens() {
        let connection = connect(Unanswering(Vec::new()), "").await;
        let opener = Opener { handle: connection.handle().clone(), liveness: connection.liveness().clone() };
        let opened = opener.open("db".into(), 5432, SocketAddr::from(([127, 0, 0, 1], 50000))).await;
        let Err(error) = opened else { panic!("the channel opened") };
        assert!(error.downcast_ref::<tokio::time::error::Elapsed>().is_some(), "{error:#}");
    }

    /// The first change is reported at once; those right after it together, once the
    /// interval has passed.
    #[tokio::test(start_paused = true)]
    async fn reports_changes_at_most_once_an_interval() {
        let mut throttle = Throttle::default();
        let start = Instant::now();
        assert!(throttle.changed(start));
        assert_eq!(throttle.due(), None);
        assert!(!throttle.changed(start + Duration::from_millis(10)));
        assert!(!throttle.changed(start + Duration::from_millis(50)));
        assert_eq!(throttle.due(), Some(start + REPORT_INTERVAL));
        throttle.reported(start + REPORT_INTERVAL);
        assert_eq!(throttle.due(), None);
        // After a quiet interval, at once again.
        assert!(throttle.changed(start + REPORT_INTERVAL * 3));
    }
}
