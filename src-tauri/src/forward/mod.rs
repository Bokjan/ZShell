//! Port forwarding over a session's SSH connection: local (`-L`), remote (`-R`) and dynamic
//! SOCKS (`-D`).
//!
//! Each run of a rule is one task that owns everything the rule holds: its listener (for
//! remote rules, its route in the connection's `ssh::RemoteRoutes` and the server's listener) and a [`JoinSet`] of
//! the connections it carries. Stopping a rule signals the task, which releases all of it (a
//! remote rule waits for the server to cancel its listener) and then ends: once the task has
//! ended, the rule can start again on the same port. Dropping the signal stops it too. A rule
//! can also be released ([`Forwards::release_all`]): it stops listening, but the connections it
//! carries go on, given back by its task as [`InFlight`], as OpenSSH lets them run after the
//! shell exits. Rules
//! belong to a saved session, which may have several connections (tabs opened separately):
//! state changes are pushed as [`SessionEvent::Forward`] to every tab of the session through
//! its [`Hub`], whichever connection runs the rule (see `crate::connections` for how a rule
//! runs on one connection only). A generation number keeps a replaced or stopped task from
//! reporting after the fact. So it does for what each run carries ([`SessionEvent::ForwardTraffic`],
//! see `traffic`).

mod draining;
mod dynamic;
mod local;
mod remote;
pub(crate) mod socks;
#[cfg(test)]
pub(crate) mod testing;
mod traffic;

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::Context as _;
use russh::client::Msg;
use russh::Channel;
use tauri::async_runtime::JoinHandle;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::{JoinError, JoinSet};
use tokio::time::Instant;

use crate::error::Error;
use crate::model::{host_port, ForwardKind, ForwardRule};
use crate::session::{ForwardState, ForwardTraffic, SessionEvent, SessionId, SessionSink};
use crate::ssh::{Liveness, RemoteRoutes, SshHandle};
use crate::traffic::{ConnectionInfo, Counted, Reporter};
pub use draining::{Draining, InFlight};
use traffic::RuleTraffic;

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
    stop: watch::Sender<Signal>,
    task: JoinHandle<InFlight>,
}

/// What a rule's task is asked to do.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Signal {
    Run,
    /// Stop, ending the connections it carries.
    Stop,
    /// Stop listening, and give back the connections it carries (see [`Forwards::release_all`]).
    Release,
}

/// A rule task's side of its stop signal.
pub(super) struct Stopping(watch::Receiver<Signal>);

impl Stopping {
    /// Completes once the rule is asked to stop (or its owner is gone).
    async fn requested(&mut self) {
        let _ = self.0.wait_for(|signal| *signal != Signal::Run).await;
    }

    /// Whether the rule was asked to give back its connections rather than end them.
    fn releases(&self) -> bool {
        *self.0.borrow() == Signal::Release
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
    /// What each running rule carries in its current run, as last reported.
    traffic: HashMap<String, ForwardTraffic>,
}

impl Hub {
    fn report(&self, rule_id: &str, state: ForwardState) {
        let mut watchers = self.0.lock().unwrap();
        for (_, sink) in &watchers.sinks {
            sink.event(SessionEvent::Forward { rule_id: rule_id.to_owned(), state: state.clone() });
        }
        // A new run starts counting afresh.
        if !matches!(state, ForwardState::Active { .. }) {
            watchers.traffic.remove(rule_id);
        }
        watchers.states.insert(rule_id.to_owned(), state);
    }

    fn report_traffic(&self, rule_id: &str, traffic: ForwardTraffic) {
        let mut watchers = self.0.lock().unwrap();
        for (_, sink) in &watchers.sinks {
            sink.event(SessionEvent::ForwardTraffic { rule_id: rule_id.to_owned(), traffic: traffic.clone() });
        }
        watchers.traffic.insert(rule_id.to_owned(), traffic);
    }

    fn print(&self, text: &str) {
        for (_, sink) in &self.0.lock().unwrap().sinks {
            sink.print(text);
        }
    }
}

struct Shared {
    handle: Arc<SshHandle>,
    info: ConnectionInfo,
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
            info: connection.info(),
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

    /// Reports rule states (and what they carry) to session `id` from now on, starting with
    /// the current ones.
    pub fn attach(&self, id: SessionId, sink: SessionSink) {
        let mut watchers = self.0.hub.0.lock().unwrap();
        for (rule_id, state) in &watchers.states {
            sink.event(SessionEvent::Forward { rule_id: rule_id.clone(), state: state.clone() });
        }
        for (rule_id, traffic) in &watchers.traffic {
            sink.event(SessionEvent::ForwardTraffic { rule_id: rule_id.clone(), traffic: traffic.clone() });
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

    /// How many connections the rules running here carry, as last reported.
    pub fn connections(&self) -> usize {
        let running = self.0.running.lock().unwrap();
        let watchers = self.0.hub.0.lock().unwrap();
        let carried = |rule_id: &String| match watchers.states.get(rule_id) {
            Some(ForwardState::Active { connections, .. }) => *connections,
            _ => 0,
        };
        running.iter().filter(|(_, r)| r.generation.is_some()).map(|(rule_id, _)| carried(rule_id)).sum()
    }

    /// Starts `rule`, restarting it if it is already running. With `announce`, a failure to
    /// start is also printed in the terminal (for rules started automatically on connect).
    pub fn start(&self, rule: ForwardRule, announce: bool) {
        let generation = self.0.next_generation.fetch_add(1, Ordering::Relaxed);
        let ctx = Ctx { shared: self.0.clone(), rule_id: rule.id.clone(), generation, traffic: Arc::new(RuleTraffic::new()) };
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
        let (stop, stopping) = watch::channel(Signal::Run);
        let task = tauri::async_runtime::spawn(run(rule.clone(), ctx, previous, announce, Stopping(stopping)));
        running.insert(rule.id.clone(), Running { rule, generation: Some(generation), stop, task });
    }

    pub fn stop(&self, rule_id: &str) {
        let mut running = self.0.running.lock().unwrap();
        if let Some(old) = running.get_mut(rule_id) {
            let _ = old.stop.send(Signal::Stop);
            old.generation = None;
        }
        self.0.report(rule_id, ForwardState::Stopped);
    }

    /// Stops every rule, as the connection closes; the session's other tabs see them stop.
    /// Returns the rules that were running, and their tasks, which end once they have
    /// released their ports and the server's listeners.
    pub fn stop_all(&self) -> (Vec<ForwardRule>, Vec<JoinHandle<InFlight>>) {
        let (rules, tasks) = self.take_all(Signal::Stop, true);
        (rules, tasks.into_iter().map(|(_, task)| task).collect())
    }

    /// Stops every rule but lets the connections they carry go on: the shell of the
    /// connection's last tab has exited. The session's other tabs see the rules stop. Returns
    /// the rules that were running, and the tasks of all the rules here, which end once they
    /// have released their ports and the server's listeners, with the connections they still
    /// carry (see [`Draining::collect`]). Rules can start here again afterwards.
    pub fn release_all(&self) -> (Vec<ForwardRule>, Vec<(ForwardRule, JoinHandle<InFlight>)>) {
        self.take_all(Signal::Release, false)
    }

    /// Signals every rule, reporting those that were running as stopped; `close`: nothing
    /// starts here any more.
    fn take_all(&self, signal: Signal, close: bool) -> (Vec<ForwardRule>, Vec<(ForwardRule, JoinHandle<InFlight>)>) {
        let mut rules = Vec::new();
        let mut tasks = Vec::new();
        let mut running = self.0.running.lock().unwrap();
        if close {
            self.0.closed.store(true, Ordering::Relaxed);
        }
        for (rule_id, old) in running.drain() {
            // A stopped rule's task has its signal already, and gives back nothing.
            if old.generation.is_some() {
                // Whether it ended by a failure, asked before the signal: once signalled, its
                // task may end on another thread at once.
                let ended = old.task.inner().is_finished();
                let _ = old.stop.send(signal);
                if !ended {
                    self.0.report(&rule_id, ForwardState::Stopped);
                    rules.push(old.rule.clone());
                }
            }
            tasks.push((old.rule, old.task));
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

/// What a rule's task needs: the connection, and a way to report its own state and what its
/// run carries.
#[derive(Clone)]
struct Ctx {
    shared: Arc<Shared>,
    rule_id: String,
    generation: u64,
    traffic: Arc<RuleTraffic>,
}

impl Ctx {
    fn handle(&self) -> Arc<SshHandle> {
        self.shared.handle.clone()
    }

    fn liveness(&self) -> Liveness {
        self.shared.liveness.clone()
    }

    /// What opens the channels of the connections a local or dynamic rule takes.
    fn opener(&self) -> Opener {
        Opener { handle: self.shared.handle.clone(), liveness: self.shared.liveness.clone(), traffic: self.traffic.clone() }
    }

    fn report(&self, state: ForwardState) {
        // Checked under the lock that `start` / `stop` hold while replacing a task, so a
        // superseded task can never overwrite the state of its successor.
        let running = self.shared.running.lock().unwrap();
        if running.get(&self.rule_id).is_some_and(|r| r.generation == Some(self.generation)) {
            self.shared.report(&self.rule_id, state);
        }
    }

    /// Reports what the run carries, as [`Ctx::report`] does its state.
    fn report_traffic(&self) {
        let running = self.shared.running.lock().unwrap();
        if running.get(&self.rule_id).is_some_and(|r| r.generation == Some(self.generation)) {
            self.shared.hub.report_traffic(&self.rule_id, self.traffic.report(self.shared.info));
        }
    }
}

/// A run of `rule`, which gives back the connections it still carries if it was released.
async fn run(rule: ForwardRule, ctx: Ctx, previous: Option<JoinHandle<InFlight>>, announce: bool, mut stopping: Stopping) -> InFlight {
    // Let the previous run release its port (and, for remote rules, the server's listener)
    // before binding it again. Its connections end.
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    let reporting = ctx.clone();
    let _reporter = Reporter::start(&ctx.traffic.wake, move || reporting.report_traffic());
    // Ends with `Ok` once stopped.
    let result = match rule.kind {
        ForwardKind::Local => local::run(&rule, &ctx, &mut stopping).await,
        ForwardKind::Remote => remote::run(&rule, &ctx, &mut stopping).await,
        ForwardKind::Dynamic => dynamic::run(&rule, &ctx, &mut stopping).await,
    };
    result.unwrap_or_else(|e| {
        let error = Error::from(e);
        if announce {
            let forward = describe(&rule);
            let message = t!("forward.autoStartFailed", forward = forward, error = error);
            ctx.shared.print(&format!("\r\n\x1b[33m{message}\x1b[0m\n"));
        }
        ctx.report(ForwardState::Failed { error });
        InFlight::default()
    })
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
pub(crate) struct Throttle {
    reported: Option<Instant>,
    /// A change not reported yet.
    pending: bool,
}

impl Throttle {
    /// Whether to report a change now; otherwise it is reported when [`Throttle::due`] says.
    pub(crate) fn changed(&mut self, now: Instant) -> bool {
        if self.reported.is_some_and(|reported| now < reported + REPORT_INTERVAL) {
            self.pending = true;
            return false;
        }
        self.reported(now);
        true
    }

    pub(crate) fn reported(&mut self, now: Instant) {
        self.reported = Some(now);
        self.pending = false;
    }

    /// When the change put off is due, if there is one.
    pub(crate) fn due(&self) -> Option<Instant> {
        self.pending.then(|| self.reported.map_or_else(Instant::now, |reported| reported + REPORT_INTERVAL))
    }
}

/// Completes at `deadline`; pending forever without one, for `select!`.
pub(crate) async fn at(deadline: Option<Instant>) {
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

    /// The connections it carries if the rule was released, to go on; otherwise none, and
    /// they end now.
    fn into_in_flight(self, stopping: &Stopping) -> InFlight {
        if stopping.releases() {
            InFlight(self.connections)
        } else {
            InFlight::default()
        }
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
    traffic: Arc<RuleTraffic>,
}

impl Opener {
    /// A channel to `host:port` for the connection from `peer`, within [`OPEN_TIMEOUT`]. The
    /// server answers the request, which tells that the connection is still alive. Fails with
    /// `forward.channelFailed` (with russh's error underneath) or `forward.channelTimeout`.
    async fn open(&self, host: String, port: u16, peer: SocketAddr) -> anyhow::Result<Channel<Msg>> {
        self.liveness.expect_reply();
        let target = host_port(&host, port);
        let handle = self.handle.clone();
        let mut opening =
            Box::pin(async move { handle.channel_open_direct_tcpip(host, port.into(), peer.ip().to_string(), peer.port().into()).await });
        match tokio::time::timeout(OPEN_TIMEOUT, &mut opening).await {
            Ok(opened) => opened.context(Error::new("forward.channelFailed").param("target", target)),
            Err(_) => {
                // The server may open it yet, and would keep its connection to the target with
                // the channel: closed then. Waiting ends with the SSH connection at the latest.
                tokio::spawn(async move {
                    if let Ok(channel) = opening.await {
                        let _ = channel.close().await;
                    }
                });
                Err(Error::new("forward.channelTimeout").param("target", target).into())
            }
        }
    }
}

/// Copies data both ways until both sides are done. Errors here are ordinary disconnects
/// (resets, aborted transfers) and are not reported. Each write to the channel is a packet
/// of its own: buffers larger than the default 8 KiB send fewer when there is much to copy.
///
/// What `stream` sends is checked for (see [`Checking`]): a client waiting for an answer on a
/// connection gone dead would otherwise wait until keepalives give up. What goes through the
/// channel counts on the rule's `traffic`.
async fn bridge(stream: TcpStream, channel: Channel<Msg>, liveness: Liveness, traffic: Arc<RuleTraffic>) {
    const BUFFER: usize = 64 << 10;
    let _ = stream.set_nodelay(true);
    let mut stream = Checking { inner: stream, liveness };
    let _carrying = traffic.carrying(channel.id().into());
    let mut channel = Counted::new(channel.into_stream(), traffic.meter.clone());
    let _ = tokio::io::copy_bidirectional_with_sizes(&mut stream, &mut channel, BUFFER, BUFFER).await;
}

/// A rule's end of a connection it carries, which checks the SSH connection whenever it reads
/// something to send through it. A check that finds something read from the server already
/// ends at once, so a busy connection costs little.
struct Checking<S> {
    inner: S,
    liveness: Liveness,
}

impl<S: AsyncRead + Unpin> AsyncRead for Checking<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.liveness.expect_reply();
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Checking<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
