//! The debug log: what the backend logs through the `log` crate (ours, russh's, russh-sftp's)
//! and what the pages report (console warnings and errors, uncaught errors, failed commands),
//! kept in memory for the debug log window (see ARCHITECTURE.md). Recorded whether or not the
//! window is open, so that it shows how a connection that just failed went; only what fits the
//! limits below is kept, the oldest going first.
//!
//! Records are untranslated English, as OpenSSH's `debug1:` lines. They never carry what is
//! typed or shown in a terminal, passwords, passphrases, the answers to keyboard-interactive
//! prompts or keys: russh's records that would (or that come with every packet) are dropped
//! ([`wanted`]).
//!
//! A record about a connection carries its number, the one the status bar shows: logged in a
//! task working on it ([`about`]: connecting, authenticating, the session's shell), or naming it
//! with the `conn` key (`log::debug!(conn = id; "...")`) from elsewhere. russh's own records
//! come from its connection task, which can't.
//!
//! While the window is open (it took a [`snapshot`]), new records also wait to be sent to it,
//! together every [`DELIVERY_INTERVAL`] (the `debug-log-added` event).

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tokio::sync::Notify;
use ts_rs::TS;

use crate::events;

#[cfg(test)]
mod tests;

/// The records each side keeps at most: the backend's take 50 to 100 for each connection; the
/// pages report little but errors.
pub(crate) const BACKEND_LINES: usize = 10_000;
pub(crate) const FRONTEND_LINES: usize = 2_000;
/// The bytes of records each side keeps at most, whichever limit comes first.
const MAX_BYTES: usize = 4 << 20;
/// A longer message is cut short (a `{:?}` of a whole buffer, say).
const MAX_MESSAGE: usize = 4 << 10;
/// What each record takes besides its text, roughly, for [`MAX_BYTES`].
const OVERHEAD: usize = 64;
/// How long new records gather before they are sent to the window together.
const DELIVERY_INTERVAL: Duration = Duration::from_millis(100);

/// Our crate's module paths start with this, which the records leave out.
const CRATE: &str = "zshell_lib::";

/// How much a record matters, as `log`'s levels; `trace` is never kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "DebugLogLevel")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    fn of(level: Level) -> Self {
        match level {
            Level::Error => Self::Error,
            Level::Warn => Self::Warn,
            Level::Info => Self::Info,
            Level::Debug | Level::Trace => Self::Debug,
        }
    }
}

/// Which side a record comes from: the window shows each in its own tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "DebugLogSource")]
pub enum LogSource {
    Backend,
    Frontend,
}

/// A record of the debug log.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "DebugLogEntry")]
pub struct LogEntry {
    /// Increases with each record, of either side.
    pub seq: u64,
    pub source: LogSource,
    /// When, in milliseconds since the Unix epoch.
    pub time: u64,
    pub level: LogLevel,
    /// The backend's module (`ssh::auth`, `russh::client::kex`), or the page's window (`main`).
    pub target: String,
    /// The connection the record is about, as the status bar numbers them.
    pub connection: Option<u32>,
    pub message: String,
}

impl LogEntry {
    fn size(&self) -> usize {
        self.target.len() + self.message.len() + OVERHEAD
    }
}

/// A page's record, sent with `debug_log_write`; the page is its target.
#[derive(Debug, Deserialize, TS)]
#[ts(rename = "DebugLogRecord")]
pub struct FrontendRecord {
    pub level: LogLevel,
    /// When, in milliseconds since the Unix epoch, by the page's clock.
    pub time: u64,
    pub message: String,
}

/// One side's records, oldest first, within `lines` and [`MAX_BYTES`].
struct Ring {
    entries: VecDeque<LogEntry>,
    bytes: usize,
    lines: usize,
}

impl Ring {
    const fn new(lines: usize) -> Self {
        Self { entries: VecDeque::new(), bytes: 0, lines }
    }

    fn push(&mut self, entry: LogEntry) {
        self.bytes += entry.size();
        self.entries.push_back(entry);
        while self.entries.len() > self.lines || self.bytes > MAX_BYTES {
            let Some(oldest) = self.entries.pop_front() else { break };
            self.bytes -= oldest.size();
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

struct State {
    next_seq: u64,
    backend: Ring,
    frontend: Ring,
    /// Records the open window hasn't been sent yet; `None` while it isn't open.
    pending: Option<Vec<LogEntry>>,
}

impl State {
    fn ring(&mut self, source: LogSource) -> &mut Ring {
        match source {
            LogSource::Backend => &mut self.backend,
            LogSource::Frontend => &mut self.frontend,
        }
    }

    /// Records `entry` (its `seq` given here); whether the window waits for it.
    fn push(&mut self, mut entry: LogEntry) -> bool {
        entry.seq = self.next_seq;
        self.next_seq += 1;
        if let Some(pending) = &mut self.pending {
            // A window that stopped taking them (its page hung) doesn't grow this for good:
            // it keeps at most what it would show.
            if pending.len() < BACKEND_LINES + FRONTEND_LINES {
                pending.push(entry.clone());
            }
        }
        let watched = self.pending.is_some();
        self.ring(entry.source).push(entry);
        watched
    }
}

struct DebugLog {
    state: Mutex<State>,
    /// Wakes the delivery to the window (see [`deliver`]).
    wake: Notify,
}

static LOG: LazyLock<DebugLog> = LazyLock::new(|| DebugLog {
    state: Mutex::new(State { next_seq: 1, backend: Ring::new(BACKEND_LINES), frontend: Ring::new(FRONTEND_LINES), pending: None }),
    wake: Notify::new(),
});

tokio::task_local! {
    /// The connection the task works on (see [`about`]).
    static CONNECTION: u32;
}

/// Runs `work` on connection `id`: what it logs is about that connection, unless a record names
/// another (or `work` runs some of it on another, a jump host).
pub async fn about<F: Future>(id: u32, work: F) -> F::Output {
    CONNECTION.scope(id, work).await
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

/// `message` cut short to [`MAX_MESSAGE`] bytes (at a character's boundary), saying so.
fn cut(mut message: String) -> String {
    if message.len() <= MAX_MESSAGE {
        return message;
    }
    let total = message.len();
    let mut end = MAX_MESSAGE;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message.push_str(&format!(" [cut short: {total} bytes]"));
    message
}

fn record(entry: LogEntry) {
    let watched = LOG.state.lock().unwrap().push(entry);
    if watched {
        LOG.wake.notify_one();
    }
}

/// Whether a record is kept: ours and russh's (and russh-sftp's) to `debug`, other crates'
/// (Tauri's, the system libraries') from `warn` only, and none of russh's that come with every
/// packet or window adjustment, which would push everything else out within minutes of typing,
/// or that dump what the agent signs and answers.
fn wanted(target: &str, level: Level, message: impl FnOnce() -> String) -> bool {
    let limit = if target.starts_with(CRATE) {
        Level::Debug
    } else if ["russh::sshbuffer", "russh::session", "russh::cipher", "russh::keys"].iter().any(|prefix| target.starts_with(prefix)) {
        Level::Info
    } else if target.starts_with("russh") {
        Level::Debug
    } else {
        Level::Warn
    };
    if level > limit {
        return false;
    }
    if level < Level::Debug || !target.starts_with("russh") {
        return true;
    }
    // Every packet received, stderr output, window adjustments, SFTP packets; and a message
    // waiting for a channel to open, which may be its data.
    const NOISE: [&str; 6] =
        ["< msg type", "channel_extended_data", "channel_window_adjust", "sender_window_size", "packet type", "msg = "];
    let message = message();
    !NOISE.iter().any(|noise| message.starts_with(noise))
}

struct Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        wanted(metadata.target(), metadata.level(), String::new)
    }

    fn log(&self, record: &Record) {
        let target = record.target();
        // Formatted once, if `wanted` reads it.
        let mut text = None;
        if !wanted(target, record.level(), || text.insert(record.args().to_string()).clone()) {
            return;
        }
        let named = record.key_values().get(log::kv::Key::from("conn")).and_then(|value| value.to_u64()).map(|id| id as u32);
        let connection = named.or_else(|| CONNECTION.try_with(|id| *id).ok());
        let message = text.unwrap_or_else(|| record.args().to_string());
        // While developing (`pnpm tauri dev`), warnings and errors are seen in the terminal too.
        #[cfg(debug_assertions)]
        if record.level() <= Level::Warn {
            eprintln!("[{}] {target}: {message}", record.level());
        }
        record_entry(LogSource::Backend, target.strip_prefix(CRATE).unwrap_or(target).to_owned(), record.level(), connection, message);
    }

    fn flush(&self) {}
}

fn record_entry(source: LogSource, target: String, level: Level, connection: Option<u32>, message: String) {
    record(LogEntry { seq: 0, source, time: now(), level: LogLevel::of(level), target, connection, message: cut(message) });
}

/// Starts recording the backend's records. First thing at startup, so that nothing logged is
/// lost; a second call (tests) changes nothing.
pub fn start() {
    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

/// Records what window `window`'s page reports.
pub fn write(window: &str, records: Vec<FrontendRecord>) {
    for FrontendRecord { level, time, message } in records {
        record(LogEntry {
            seq: 0,
            source: LogSource::Frontend,
            time,
            level,
            target: window.to_owned(),
            connection: None,
            message: cut(message),
        });
    }
}

/// Both sides' records, in order, for the window, which from then on is sent the new ones too.
pub fn snapshot() -> Vec<LogEntry> {
    let mut state = LOG.state.lock().unwrap();
    state.pending = Some(Vec::new());
    let mut entries: Vec<LogEntry> = state.backend.entries.iter().chain(&state.frontend.entries).cloned().collect();
    entries.sort_unstable_by_key(|entry| entry.seq);
    entries
}

/// The window's page is gone: nothing more is sent to it.
pub fn unwatch() {
    LOG.state.lock().unwrap().pending = None;
}

/// Forgets one side's records.
pub fn clear(source: LogSource) {
    let mut state = LOG.state.lock().unwrap();
    state.ring(source).clear();
    if let Some(pending) = &mut state.pending {
        pending.retain(|entry| entry.source != source);
    }
}

/// Sends the open window what is recorded, every [`DELIVERY_INTERVAL`] at most.
pub fn deliver(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            LOG.wake.notified().await;
            tokio::time::sleep(DELIVERY_INTERVAL).await;
            let entries = LOG.state.lock().unwrap().pending.as_mut().map(std::mem::take).unwrap_or_default();
            if !entries.is_empty() {
                events::emit_to(&app, crate::window::DEBUG_LOG, events::DebugLogAdded(entries));
            }
        }
    });
}
