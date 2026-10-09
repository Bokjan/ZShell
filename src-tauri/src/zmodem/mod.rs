//! ZMODEM file transfers (`rz` / `sz`) inside a terminal session, for hosts without SFTP
//! (typically behind bastion hosts). Works the same for SSH and local sessions: it sits on
//! the session's output and input, not on a backend.
//!
//! The session's output passes through [`Zmodem::output`], which watches for the header
//! `sz` (ZRQINIT) or `rz` (ZRINIT) starts with. From there until the transfer ends, output
//! goes to the transfer task instead of the terminal, keystrokes are dropped (Ctrl+C cancels),
//! and the task's own data goes out through the session's input (see `TermIo::recv`). The
//! frontend is asked where to save or what to send through session events, and the progress
//! is written into the terminal.

mod frame;
mod link;
mod receive;
mod send;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use serde::Serialize;
use ts_rs::TS;
use tokio::sync::{mpsc, oneshot, watch};

use crate::error::Error;
use crate::session::{SessionEvent, SessionSink};
use frame::{Kind, ZPAD};
use link::Link;

/// Outgoing chunks in flight to the backend; a sender waits beyond this (backpressure).
const OUTGOING_CHUNKS: usize = 4;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
/// How long the header that started a transfer may take to arrive in full; output that only
/// looked like the start of one (`cat` of a binary file) is held back this long at most.
const START_TIMEOUT: Duration = Duration::from_secs(2);

/// What the frontend is asked for, or that a transfer is running (for its cancel button).
#[derive(Clone, Copy, Serialize, TS)]
#[ts(rename = "ZmodemPhase")]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// `sz` is about to send: where should files go?
    ChooseDestination,
    /// `rz` is waiting: which files to send?
    ChooseFiles,
    Transferring,
    Idle,
}

/// The frontend's answer to a [`Phase::ChooseDestination`] or [`Phase::ChooseFiles`].
pub enum Reply {
    Destination(PathBuf),
    Files(Vec<PathBuf>),
}

/// Receives the transfer's progress, for display.
pub trait Report {
    fn start(&mut self, name: &str, size: Option<u64>);
    fn progress(&mut self, done: u64);
    fn received(&mut self, path: &Path, size: u64);
    fn sent(&mut self, name: &str, size: u64);
    /// The receiver declined a file we offered.
    fn skipped(&mut self, name: &str);
    /// A file failed without ending the transfer.
    fn failed(&mut self, error: anyhow::Error);
}

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    /// The remote runs `sz`.
    Receive,
    /// The remote runs `rz`.
    Send,
}

enum State {
    /// Watching the output. `tail` is the end of what was scanned, in case a header starts
    /// there and continues in the next chunk; its last `held` bytes look like the start of a
    /// header and are not shown yet.
    Idle { tail: Vec<u8>, held: usize },
    Active(Active),
}

struct Active {
    incoming: mpsc::UnboundedSender<Vec<u8>>,
    cancel: watch::Sender<bool>,
    /// Waiting for the frontend's answer.
    reply: Option<oneshot::Sender<Reply>>,
}

/// One session's ZMODEM state, shared by its output path, its input and the frontend.
pub struct Zmodem {
    state: Mutex<State>,
    outgoing: mpsc::Sender<Vec<u8>>,
}

impl Zmodem {
    /// Also returns the receiving end of what transfers send, for the session's input.
    pub fn new() -> (Arc<Self>, mpsc::Receiver<Vec<u8>>) {
        let (outgoing, outgoing_rx) = mpsc::channel(OUTGOING_CHUNKS);
        (Arc::new(Self { state: Mutex::new(State::Idle { tail: Vec::new(), held: 0 }), outgoing }), outgoing_rx)
    }

    /// Takes the remote side's output; returns the part the terminal should show (when a
    /// transfer starts, what came before it is shown here already).
    pub fn output(self: &Arc<Self>, bytes: Vec<u8>, sink: &SessionSink) -> Vec<u8> {
        let mut state = self.state.lock().unwrap();
        self.scan(&mut state, bytes, sink)
    }

    fn scan(self: &Arc<Self>, state: &mut State, bytes: Vec<u8>, sink: &SessionSink) -> Vec<u8> {
        let (tail, held) = match state {
            State::Active(active) => {
                let _ = active.incoming.send(bytes);
                return Vec::new();
            }
            State::Idle { tail, held } => (tail, held),
        };
        // How much of `scan` the terminal has shown.
        let shown = tail.len() - *held;
        let mut scan = std::mem::take(tail);
        scan.extend_from_slice(&bytes);
        let Some((start, direction)) = detect(&scan) else {
            let hold = partial_header(&scan);
            let show_until = (scan.len() - hold).max(shown);
            *held = scan.len() - show_until;
            *tail = scan[scan.len().saturating_sub((frame::ZRQINIT_SIGNATURE.len() - 1).max(*held))..].to_vec();
            return scan[shown..show_until].to_vec();
        };
        let (incoming, incoming_rx) = mpsc::unbounded_channel();
        let _ = incoming.send(scan[start..].to_vec());
        let (cancel, cancel_rx) = watch::channel(false);
        *state = State::Active(Active { incoming, cancel, reply: None });
        let mut link = Link::new(incoming_rx, self.outgoing.clone(), cancel_rx);
        link.charset = sink.encoding();
        link.record(shown.saturating_sub(start));
        // What came before the transfer is shown, and the decoded text ended (a character cut
        // off by the transfer won't be completed), before the transfer task writes anything.
        let before = scan[shown.min(start)..start].to_vec();
        if !before.is_empty() {
            sink.remote(before);
        }
        sink.flush_text();
        tauri::async_runtime::spawn(run(self.clone(), direction, link, sink.clone()));
        Vec::new()
    }

    pub fn is_active(&self) -> bool {
        matches!(*self.state.lock().unwrap(), State::Active(_))
    }

    /// Keystrokes during a transfer: Ctrl+C cancels, anything else is dropped.
    pub fn input(&self, data: &[u8]) {
        if data.contains(&0x03) {
            self.cancel();
        }
    }

    pub fn cancel(&self) {
        if let State::Active(active) = &*self.state.lock().unwrap() {
            let _ = active.cancel.send(true);
        }
    }

    /// Passes on the frontend's answer, if a transfer is waiting for one.
    pub fn reply(&self, reply: Reply) {
        if let State::Active(active) = &mut *self.state.lock().unwrap() {
            if let Some(sender) = active.reply.take() {
                let _ = sender.send(reply);
            }
        }
    }

    /// Prepares to receive the frontend's answer; ask (send the event) after this.
    fn expect_reply(&self) -> oneshot::Receiver<Reply> {
        let (sender, receiver) = oneshot::channel();
        if let State::Active(active) = &mut *self.state.lock().unwrap() {
            active.reply = Some(sender);
        }
        receiver
    }

    /// Back to watching the output. What arrived after the transfer goes to the terminal;
    /// taken under the lock, so no output can slip into the finished transfer meanwhile.
    /// After a transfer it is watched as well, as the next one may follow at once (`sz a;
    /// sz b`); output that only looked like a transfer is shown as it is (`detect` false), or
    /// it would be taken for one again.
    fn finish(self: &Arc<Self>, link: &mut Link, sink: &SessionSink, detect: bool) {
        let mut state = self.state.lock().unwrap();
        let rest = link.take_rest();
        *state = State::Idle { tail: Vec::new(), held: 0 };
        let shown = if detect { self.scan(&mut state, rest, sink) } else { rest };
        if !shown.is_empty() {
            sink.remote(shown);
        }
    }
}

/// Where a transfer starts in `output`, and which way it goes. Includes the second ZPAD
/// that lrzsz sends, so none of the header shows in the terminal.
fn detect(output: &[u8]) -> Option<(usize, Direction)> {
    let len = frame::ZRQINIT_SIGNATURE.len();
    let (start, direction) = output.windows(len).enumerate().find_map(|(i, window)| {
        if window == frame::ZRQINIT_SIGNATURE {
            Some((i, Direction::Receive))
        } else if window == frame::ZRINIT_SIGNATURE {
            Some((i, Direction::Send))
        } else {
            None
        }
    })?;
    let start = if start > 0 && output[start - 1] == ZPAD { start - 1 } else { start };
    Some((start, direction))
}

/// How many bytes at the end of `output` may be the start of a header (`*`, ZDLE, `B`, `0`),
/// to hold back until the next chunk shows whether they are: on a serial line a header often
/// arrives in pieces, and none of it should show. Only once a ZDLE is among them, which
/// ordinary output doesn't end with, so a prompt ending in `*` is not held back.
fn partial_header(output: &[u8]) -> usize {
    let signature = frame::ZRQINIT_SIGNATURE;
    let Some(n) = (2..signature.len()).rev().find(|&n| output.ends_with(&signature[..n])) else { return 0 };
    // The second ZPAD that lrzsz sends.
    if output.len() > n && output[output.len() - n - 1] == ZPAD {
        n + 1
    } else {
        n
    }
}

/// The transfer task.
async fn run(zmodem: Arc<Zmodem>, direction: Direction, mut link: Link, sink: SessionSink) {
    let mut report = TerminalReport::new(sink.clone(), direction);
    let mut started = true;
    match transfer(&zmodem, direction, &mut link, &sink, &mut report).await {
        Ok(()) => {}
        Err(e) => {
            let code = e.downcast_ref::<Error>().map(Error::code);
            if code == Some("zmodem.notStarted") {
                // Output that only looked like a transfer (e.g. `cat` of a binary file).
                started = false;
            } else {
                if code != Some("zmodem.remoteCancelled") {
                    link.abort().await;
                }
                // The data still in flight is not for the terminal.
                let after = match direction {
                    Direction::Receive => link.drain(Duration::from_millis(300), Duration::from_secs(3)).await,
                    Direction::Send => Vec::new(),
                };
                report.error(e);
                // Typically the shell prompt, once the remote program has exited.
                let after: Vec<u8> = after.into_iter().skip_while(|&b| b == 0x08).collect();
                if !after.is_empty() {
                    sink.remote(after);
                }
            }
        }
    }
    sink.event(SessionEvent::Zmodem { phase: Phase::Idle });
    zmodem.finish(&mut link, &sink, started);
}

async fn transfer(zmodem: &Zmodem, direction: Direction, link: &mut Link, sink: &SessionSink, report: &mut TerminalReport) -> Result<()> {
    // The header that started it, in full, and of the expected kind.
    // If it isn't one, what was read goes to the terminal after all (see `Zmodem::finish`).
    let expected = if direction == Direction::Receive { Kind::Rqinit } else { Kind::Rinit };
    let first = match tokio::time::timeout(START_TIMEOUT, link.header(START_TIMEOUT)).await {
        Ok(Ok(header)) if header.kind == expected => header,
        Ok(Err(e)) if e.downcast_ref::<Error>().is_some_and(|e| e.code() == "zmodem.cancelled") => {
            // Ctrl+C before anything showed it was a transfer: meant for the remote program.
            link.send_now(&[0x03]).await;
            bail!(Error::new("zmodem.notStarted"));
        }
        _ => bail!(Error::new("zmodem.notStarted")),
    };
    link.stop_recording();
    // Start on a fresh line, below what the remote printed (`rz waiting to receive.`).
    sink.write(b"\r\n".to_vec());

    let reply = zmodem.expect_reply();
    let phase = if direction == Direction::Receive { Phase::ChooseDestination } else { Phase::ChooseFiles };
    sink.event(SessionEvent::Zmodem { phase });
    let reply = link.wait_for(reply).await?;
    sink.event(SessionEvent::Zmodem { phase: Phase::Transferring });
    match (direction, reply) {
        (Direction::Receive, Ok(Reply::Destination(dir))) => receive::receive(link, &dir, report).await,
        (Direction::Send, Ok(Reply::Files(paths))) if !paths.is_empty() => {
            // The receiver's repeated ZRINITs while the user was choosing.
            link.take_rest();
            send::send(link, first, &paths, report).await
        }
        _ => bail!(Error::new("zmodem.cancelled")),
    }
}

/// Progress lines in the terminal, rewritten in place, and a line per finished file.
struct TerminalReport {
    sink: SessionSink,
    direction: Direction,
    name: String,
    size: Option<u64>,
    done: u64,
    started: Instant,
    last: Instant,
    /// A progress line is showing and should be replaced.
    pending: bool,
}

impl TerminalReport {
    fn new(sink: SessionSink, direction: Direction) -> Self {
        let now = Instant::now();
        Self { sink, direction, name: String::new(), size: None, done: 0, started: now, last: now, pending: false }
    }

    fn progress_line(&mut self) {
        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let speed = format_size((self.done as f64 / elapsed) as u64);
        let (name, done) = (&self.name, format_size(self.done));
        let line = match (self.size, self.direction) {
            (Some(size), Direction::Receive) => {
                t!("zmodem.receiving", name = name, done = done, total = format_size(size), percent = percent(self.done, size), speed = speed)
            }
            (Some(size), Direction::Send) => {
                t!("zmodem.sending", name = name, done = done, total = format_size(size), percent = percent(self.done, size), speed = speed)
            }
            (None, _) => t!("zmodem.receivingUnknownSize", name = name, done = done, speed = speed),
        };
        self.sink.write(format!("\r\x1b[K\x1b[2m{line}\x1b[0m").into_bytes());
        self.pending = true;
        self.last = Instant::now();
    }

    /// Replaces the progress line with a final one.
    fn line(&mut self, text: &str, color: &str) {
        let clear = if self.pending { "\r\x1b[K" } else { "" };
        self.sink.write(format!("{clear}{color}{text}\x1b[0m\r\n").into_bytes());
        self.pending = false;
    }

    fn error(&mut self, error: anyhow::Error) {
        let error = Error::from(error);
        let color = if error.code() == "zmodem.cancelled" { "\x1b[33m" } else { "\x1b[31m" };
        self.line(&error.to_string(), color);
    }
}

impl Report for TerminalReport {
    fn start(&mut self, name: &str, size: Option<u64>) {
        self.name = name.to_owned();
        self.size = size;
        self.done = 0;
        self.started = Instant::now();
        self.progress_line();
    }

    fn progress(&mut self, done: u64) {
        self.done = done;
        if self.last.elapsed() >= PROGRESS_INTERVAL {
            self.progress_line();
        }
    }

    fn received(&mut self, path: &Path, size: u64) {
        let text = t!("zmodem.received", name = self.name, size = format_size(size), path = path.display());
        self.line(&text, "");
    }

    fn sent(&mut self, name: &str, size: u64) {
        self.line(&t!("zmodem.sent", name = name, size = format_size(size)), "");
    }

    fn skipped(&mut self, name: &str) {
        self.line(&t!("zmodem.skipped", name = name), "\x1b[33m");
    }

    fn failed(&mut self, error: anyhow::Error) {
        self.line(&Error::from(error).to_string(), "\x1b[31m");
    }
}

fn percent(done: u64, size: u64) -> u64 {
    (done.min(size) * 100).checked_div(size).unwrap_or(100)
}

/// Binary-prefixed size, as the frontend shows sizes: "512 B", "1.5 MB", "12 GB".
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
