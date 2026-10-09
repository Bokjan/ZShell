//! Terminal sessions.
//!
//! A session is a background task that owns a [`TermIo`]: it streams output bytes to the
//! frontend through a [`tauri::ipc::Channel`], reports lifecycle [`SessionEvent`]s, and
//! consumes [`SessionInput`] (keystrokes, resizes) from the frontend. Closing a session
//! aborts its task.
//!
//! Port forwards report their state through the same event channel ([`SessionSink`]).
//!
//! Backends: remote shells over SSH (`ssh::run`), Telnet (`telnet::run`), serial lines
//! (`serial::run`) and local shells in a pseudo terminal (`pty::run`).
//!
//! Remote output goes through [`SessionSink::output`], where ZMODEM transfers are detected and
//! take over the session until they end (see [`crate::zmodem`]). What the terminal shows is then
//! decoded from the session's character encoding ([`SessionSink::remote`]), and keystrokes are
//! encoded on the way back ([`TermIo::recv`]); ZMODEM data and our own messages are not.
//!
//! Output is flow controlled: the frontend acknowledges the bytes xterm.js has processed
//! ([`SessionManager::ack`]), and backends that can produce output faster than the terminal
//! renders it (local PTYs) wait on [`Flow`] before reading more.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use crate::encoding::Codec;
use crate::error::{Error, Result};
use crate::forward::ForwardState;
use crate::logging::LogSlot;
use crate::zmodem::{self, Zmodem};

pub type SessionId = u32;

#[derive(Debug)]
pub enum SessionInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    /// A break signal: on a serial line, or Telnet's `BRK`. Other backends ignore it.
    Break,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    Connected,
    /// `status`: the exit status of the shell, if it reported one.
    Closed { reason: CloseReason, error: Option<Error>, status: Option<u32> },
    Forward { rule_id: String, state: ForwardState },
    /// A ZMODEM transfer needs an answer, is running, or has ended.
    Zmodem { phase: zmodem::Phase },
    /// The session's log started (`path`), stopped (neither), or couldn't start (`error`).
    Log { path: Option<String>, error: Option<Error> },
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseReason {
    /// The shell exited or the remote side closed the session.
    Exited,
    /// The connection broke after the session had started.
    Lost,
    /// Connecting, authenticating or starting the shell failed.
    Failed,
}

/// How a remote session (SSH, Telnet, serial) ended; see [`TermIo::finish`].
pub enum Outcome {
    /// The remote side closed the session, with the shell's exit status if it reported one.
    Exited(Option<u32>),
    /// The connection broke (or the device went away) after the session had started.
    Lost(Error),
    /// Connecting, authenticating or starting the session failed.
    Failed(Error),
}

/// Pause reading once this many output bytes are unacknowledged...
const FLOW_HIGH: usize = 2 << 20;
/// ...and resume once the frontend has caught up to this many.
const FLOW_LOW: usize = 512 << 10;

/// Output flow control: counts the bytes sent to the frontend and not yet acknowledged.
#[derive(Default)]
pub struct Flow {
    state: Mutex<FlowState>,
    resumed: Condvar,
}

#[derive(Default)]
struct FlowState {
    unacked: usize,
    closed: bool,
}

impl Flow {
    fn sent(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.unacked = state.unacked.saturating_add(bytes);
    }

    pub fn ack(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.unacked = state.unacked.saturating_sub(bytes);
        if state.unacked <= FLOW_LOW {
            self.resumed.notify_all();
        }
    }

    /// Blocks while the frontend is too far behind. For backends reading on their own
    /// thread; returns immediately once the session is closing.
    pub fn wait_ready(&self) {
        let state = self.state.lock().unwrap();
        if state.unacked < FLOW_HIGH {
            return;
        }
        let _state = self.resumed.wait_while(state, |s| s.unacked > FLOW_LOW && !s.closed).unwrap();
    }

    /// Stops all waiting, for good (the session is closing).
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.resumed.notify_all();
    }
}

/// Output side of a session: terminal bytes and lifecycle events. Cloneable, so features
/// running beside the shell (port forwarding) can report to the same tab.
#[derive(Clone)]
pub struct SessionSink {
    output: Channel,
    events: Channel<SessionEvent>,
    flow: Arc<Flow>,
    zmodem: Arc<Zmodem>,
    log: Arc<LogSlot>,
    /// The session's character encoding; `None` for UTF-8.
    codec: Option<Arc<Codec>>,
}

impl SessionSink {
    /// Output from the remote side (or the local shell): shown in the terminal, unless a
    /// ZMODEM transfer starts or is running.
    pub fn output(&self, bytes: Vec<u8>) {
        let shown = self.zmodem.output(bytes, self);
        if !shown.is_empty() {
            self.remote(shown);
        }
    }

    /// Remote output for the terminal, past ZMODEM detection: converted to UTF-8 first.
    pub fn remote(&self, bytes: Vec<u8>) {
        match &self.codec {
            Some(codec) => self.write(codec.decode(&bytes)),
            None => self.write(bytes),
        }
    }

    /// The remote side's character encoding (for file names in ZMODEM transfers).
    pub fn encoding(&self) -> &'static encoding_rs::Encoding {
        self.codec.as_ref().map_or(encoding_rs::UTF_8, |codec| codec.encoding())
    }

    /// Writes to the terminal directly (our own messages, or output already filtered), and
    /// to the session's log.
    pub fn write(&self, bytes: Vec<u8>) {
        self.log.write(&bytes);
        self.flow.sent(bytes.len());
        // A send error means the frontend is gone; the session will be closed shortly.
        let _ = self.output.send(InvokeResponseBody::Raw(bytes));
    }

    pub fn flow(&self) -> Arc<Flow> {
        self.flow.clone()
    }

    /// Turns off what a program may have left on when the session ends without it turning
    /// them off (the connection dropped, the program was killed): the alternate screen, which
    /// would hide our message about how the session ended, mouse reporting, and with a soft
    /// reset bracketed paste, application cursor keys, a hidden cursor and so on. Leaving the
    /// alternate screen restores the cursor saved on entering it; saving it first makes that
    /// a no-op when the normal screen is shown (xterm.js keeps one saved cursor per screen).
    /// Not logged.
    pub fn reset_modes(&self) {
        const RESET: &[u8] = b"\x1b7\x1b[?1049l\x1b[?1000l\x1b[?1006l\x1b[!p";
        self.flow.sent(RESET.len());
        let _ = self.output.send(InvokeResponseBody::Raw(RESET.to_vec()));
    }

    pub fn log(&self) -> &LogSlot {
        &self.log
    }

    /// Writes text, translating bare `\n` to `\r\n`.
    pub fn print(&self, text: &str) {
        self.write(text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes());
    }

    pub fn event(&self, event: SessionEvent) {
        let _ = self.events.send(event);
    }
}

/// Names the program running in the foreground of a session's terminal, if it is not the
/// shell itself (an empty name if it cannot be determined).
pub type ForegroundProbe = Box<dyn Fn() -> Option<String> + Send>;

/// Where a backend that can tell (local PTYs) installs its [`ForegroundProbe`], for the
/// frontend to ask before closing a tab.
#[derive(Clone, Default)]
pub struct Foreground(Arc<Mutex<Option<ForegroundProbe>>>);

impl Foreground {
    pub fn set(&self, probe: Option<ForegroundProbe>) {
        *self.0.lock().unwrap() = probe;
    }

    pub fn get(&self) -> Option<String> {
        self.0.lock().unwrap().as_ref().and_then(|probe| probe())
    }
}

/// The terminal side of a session, as seen by its backend task.
pub struct TermIo {
    sink: SessionSink,
    foreground: Foreground,
    input: mpsc::UnboundedReceiver<SessionInput>,
    /// What a ZMODEM transfer sends to the remote side.
    zmodem_out: mpsc::Receiver<Vec<u8>>,
    /// Whether the last input returned came from a ZMODEM transfer (see [`TermIo::is_transfer_data`]).
    transfer_data: bool,
    /// Latest terminal size (cols, rows) reported by the frontend.
    pub size: (u16, u16),
}

impl TermIo {
    fn new(
        output: Channel,
        events: Channel<SessionEvent>,
        size: (u16, u16),
        log: Arc<LogSlot>,
        encoding: &'static encoding_rs::Encoding,
    ) -> (Self, mpsc::UnboundedSender<SessionInput>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (zmodem, zmodem_out) = Zmodem::new();
        let codec = Codec::new(encoding).map(Arc::new);
        let sink = SessionSink { output, events, flow: Arc::default(), zmodem, log, codec };
        (Self { sink, foreground: Foreground::default(), input: rx, zmodem_out, transfer_data: false, size }, tx)
    }

    /// A session without a frontend, for backend tests: returns the input sender and the
    /// channels' receiving ends (raw output bytes, events as JSON).
    #[cfg(test)]
    pub fn detached(
        size: (u16, u16),
    ) -> (Self, mpsc::UnboundedSender<SessionInput>, std::sync::mpsc::Receiver<Vec<u8>>, std::sync::mpsc::Receiver<String>) {
        let (output_tx, output_rx) = std::sync::mpsc::channel();
        let output = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                let _ = output_tx.send(bytes);
            }
            Ok(())
        });
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let events = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let _ = events_tx.send(json);
            }
            Ok(())
        });
        let log = LogSlot::new(crate::logging::LogInfo::default());
        let (io, input) = Self::new(output, events, size, log, encoding_rs::UTF_8);
        (io, input, output_rx, events_rx)
    }

    /// Output from the remote side; see [`SessionSink::output`].
    pub fn output(&self, bytes: Vec<u8>) {
        self.sink.output(bytes);
    }

    pub fn print(&self, text: &str) {
        self.sink.print(text);
    }

    pub fn event(&self, event: SessionEvent) {
        self.sink.event(event);
    }

    /// Reports how a remote session ended, in the terminal and as its `Closed` event.
    pub fn finish(&self, outcome: Outcome) {
        self.sink.reset_modes();
        let (reason, error, status) = match outcome {
            Outcome::Exited(status) => {
                let message = match status {
                    Some(status) => t!("terminal.closedWithStatus", status = status),
                    None => t!("terminal.closed"),
                };
                self.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
                (CloseReason::Exited, None, status)
            }
            Outcome::Lost(e) => {
                // The remote program may have left the cursor mid-line.
                self.print(&format!("\n\x1b[31m{e}\x1b[0m\n"));
                (CloseReason::Lost, Some(e), None)
            }
            Outcome::Failed(e) => {
                self.print(&format!("\x1b[31m{e}\x1b[0m\n"));
                (CloseReason::Failed, Some(e), None)
            }
        };
        self.event(SessionEvent::Closed { reason, error, status });
    }

    pub fn sink(&self) -> SessionSink {
        self.sink.clone()
    }

    pub fn foreground(&self) -> Foreground {
        self.foreground.clone()
    }

    /// Receives the next input for the remote side, keeping [`TermIo::size`] up to date.
    /// During a ZMODEM transfer that is the transfer's data; keystrokes only cancel it.
    /// Keystrokes are converted to the session's encoding.
    pub async fn recv(&mut self) -> Option<SessionInput> {
        self.next_input(true).await
    }

    /// `convert`: whether keystrokes are converted to the session's encoding. Prompts read
    /// UTF-8, which is also what the SSH protocol uses for passwords and answers.
    async fn next_input(&mut self, convert: bool) -> Option<SessionInput> {
        loop {
            self.transfer_data = false;
            let input = tokio::select! {
                biased;
                Some(data) = self.zmodem_out.recv() => {
                    self.transfer_data = true;
                    return Some(SessionInput::Data(data));
                }
                input = self.input.recv() => input,
            };
            match &input {
                Some(SessionInput::Data(data)) if self.sink.zmodem.is_active() => {
                    self.sink.zmodem.input(data);
                    continue;
                }
                Some(SessionInput::Data(data)) if convert => {
                    if let Some(codec) = &self.sink.codec {
                        return Some(SessionInput::Data(codec.encode(data)));
                    }
                }
                Some(SessionInput::Resize { cols, rows }) => self.size = (*cols, *rows),
                _ => {}
            }
            return input;
        }
    }

    /// Whether the data [`TermIo::recv`] returned last is a ZMODEM transfer's rather than
    /// typing, which backends that echo typing locally (Telnet) must not echo.
    pub fn is_transfer_data(&self) -> bool {
        self.transfer_data
    }

    /// Reads one line typed into the terminal, for prompts shown before the remote shell
    /// starts. Returns `None` if the user cancels with Ctrl+C / Ctrl+D or the session closes.
    pub async fn read_line(&mut self, echo: bool) -> Option<String> {
        let mut line = String::new();
        loop {
            let SessionInput::Data(data) = self.next_input(false).await? else {
                continue;
            };
            for c in String::from_utf8_lossy(&strip_escapes(&data)).chars() {
                match c {
                    '\r' | '\n' => {
                        self.print("\n");
                        return Some(line);
                    }
                    '\x7f' | '\x08' => {
                        if let Some(removed) = line.pop() {
                            if echo {
                                self.print(&"\x08 \x08".repeat(removed.width().unwrap_or(1)));
                            }
                        }
                    }
                    '\x03' => {
                        self.print("^C\n");
                        return None;
                    }
                    '\x04' if line.is_empty() => {
                        self.print("\n");
                        return None;
                    }
                    c if !c.is_control() => {
                        line.push(c);
                        if echo {
                            self.print(c.encode_utf8(&mut [0; 4]));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `data` without escape sequences, which prompts don't support (arrow keys, Alt+key). This
/// includes the markers around a paste in bracketed paste mode, which the shell of a
/// previous connection may have left on: the pasted text itself is kept.
fn strip_escapes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut bytes = data.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != 0x1b {
            out.push(byte);
            continue;
        }
        match bytes.next() {
            // CSI: parameters up to a final byte.
            Some(b'[') => {
                for byte in bytes.by_ref() {
                    if (0x40..=0x7e).contains(&byte) {
                        break;
                    }
                }
            }
            // SS3 (application cursor keys): one more byte.
            Some(b'O') => {
                bytes.next();
            }
            _ => {}
        }
    }
    out
}

struct SessionEntry {
    input: mpsc::UnboundedSender<SessionInput>,
    sink: SessionSink,
    flow: Arc<Flow>,
    foreground: Foreground,
    zmodem: Arc<Zmodem>,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub struct SessionManager {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<SessionId, SessionEntry>>,
}

impl SessionManager {
    /// Starts a session whose backend task is produced by `backend`, logging to `log` (which
    /// may already be writing, so that the log starts with the session's first output).
    /// `encoding` is the remote side's character encoding.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn<F, Fut>(
        &self,
        output: Channel,
        events: Channel<SessionEvent>,
        size: (u16, u16),
        log: Arc<LogSlot>,
        encoding: &'static encoding_rs::Encoding,
        backend: F,
    ) -> SessionId
    where
        F: FnOnce(SessionId, TermIo) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (io, input) = TermIo::new(output, events, size, log, encoding);
        let sink = io.sink();
        let flow = io.sink.flow();
        let foreground = io.foreground();
        let zmodem = io.sink.zmodem.clone();
        let task = tauri::async_runtime::spawn(backend(id, io));
        self.sessions.lock().unwrap().insert(id, SessionEntry { input, sink, flow, foreground, zmodem, task });
        id
    }

    pub fn send(&self, id: SessionId, input: SessionInput) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let entry = sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?;
        // A send error means the backend already exited; it will be removed on close.
        let _ = entry.input.send(input);
        Ok(())
    }

    /// Records that the frontend has processed `bytes` of output.
    pub fn ack(&self, id: SessionId, bytes: usize) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let entry = sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?;
        entry.flow.ack(bytes);
        Ok(())
    }

    /// The program running in the session's foreground other than its shell, if any (see
    /// [`ForegroundProbe`]); always `None` for sessions that cannot tell.
    pub fn foreground(&self, id: SessionId) -> Result<Option<String>> {
        let foreground = {
            let sessions = self.sessions.lock().unwrap();
            sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.foreground.clone()
        };
        Ok(foreground.get())
    }

    /// The session's output side: its log, and events for its tab.
    pub fn sink(&self, id: SessionId) -> Result<SessionSink> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.sink.clone())
    }

    pub fn zmodem(&self, id: SessionId) -> Result<Arc<Zmodem>> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?.zmodem.clone())
    }

    pub fn remove(&self, id: SessionId) -> Result<()> {
        let entry = self.sessions.lock().unwrap().remove(&id).ok_or_else(|| Error::new("session.notFound"))?;
        entry.flow.close();
        entry.task.abort();
        // A transfer waiting for the user to choose files would otherwise wait for ever.
        entry.zmodem.cancel();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prompts_take_pastes_and_ignore_other_escapes() {
        let (mut io, input, _output, _events) = TermIo::detached((80, 24));
        // A bracketed paste, then an arrow key in both cursor modes and Alt+b.
        input.send(SessionInput::Data(b"\x1b[200~se\xe5\xaf\x86cret\x1b[201~".to_vec())).unwrap();
        input.send(SessionInput::Data(b"\x1b[D\x1bOD\x1bb!\r".to_vec())).unwrap();
        assert_eq!(io.read_line(false).await.as_deref(), Some("se密cret!"));
    }

    /// Collects what the terminal shows until it ends with `want` (or `wait` passes).
    fn shown_until(output: &std::sync::mpsc::Receiver<Vec<u8>>, want: &[u8], wait: std::time::Duration) -> Vec<u8> {
        let deadline = std::time::Instant::now() + wait;
        let mut shown = Vec::new();
        while !shown.ends_with(want) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match output.recv_timeout(left) {
                Ok(bytes) => shown.extend(bytes),
                Err(_) => break,
            }
        }
        shown
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn output_that_only_looks_like_zmodem_is_shown() {
        let (io, _input, output, _events) = TermIo::detached((80, 24));
        let sink = io.sink();
        // sz's signature without a header after it (`cat` of a binary file), then more output,
        // slowly but steadily.
        let mut sent = b"cat a.bin\r\n**\x18B00 not a header".to_vec();
        sink.output(sent.clone());
        for i in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let more = format!(" more {i}").into_bytes();
            sent.extend(&more);
            sink.output(more);
        }
        let shown = shown_until(&output, b" more 9", std::time::Duration::from_secs(5));
        assert_eq!(String::from_utf8_lossy(&shown), String::from_utf8_lossy(&sent));
        assert!(!sink.zmodem.is_active());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ctrl_c_before_a_transfer_starts_reaches_the_remote_program() {
        let (mut io, _input, output, _events) = TermIo::detached((80, 24));
        let sink = io.sink();
        sink.output(b"**\x18B00".to_vec());
        assert!(sink.zmodem.is_active());
        sink.zmodem.input(b"\x03");
        let sent = tokio::time::timeout(std::time::Duration::from_secs(2), io.recv()).await.unwrap();
        assert!(matches!(sent, Some(SessionInput::Data(data)) if data == b"\x03"));
        assert_eq!(shown_until(&output, b"**\x18B00", std::time::Duration::from_secs(2)), b"**\x18B00");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn closing_the_session_ends_a_transfer_waiting_for_the_user() {
        let (io, _input, _output, events) = TermIo::detached((80, 24));
        let sink = io.sink();
        sink.output(b"rz waiting to receive.**\x18B0100000023be50\r\x8a\x11".to_vec());
        let asked = events.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert!(asked.contains("chooseFiles"), "{asked}");
        drop(io);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while sink.zmodem.is_active() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!sink.zmodem.is_active());
    }
}
