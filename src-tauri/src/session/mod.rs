//! Terminal sessions.
//!
//! A session is a background task that owns a [`TermIo`]: it streams output bytes to the
//! frontend through a [`tauri::ipc::Channel`], reports lifecycle [`SessionEvent`]s, and
//! consumes [`SessionInput`] (keystrokes, resizes) from the frontend.
//!
//! [`SessionManager::remove`] is the only way a session closes (the tab closing, its page
//! reloading, its window going away): it releases what the session holds, then aborts its
//! task. What a session holds beyond its task registers its release on the session's
//! [`Lifetime`], which also runs when the session ends by itself ([`TermIo::finish`]) or its
//! task is dropped, whichever comes first, so nothing depends on the order of these.
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
//! ([`SessionManager::ack`]), and every backend waits on [`Flow`] before reading more while
//! the terminal is too far behind (the remote side is then held back by TCP, the SSH window
//! or the serial line's flow control).

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde::Serialize;
use ts_rs::TS;
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

#[derive(Clone, Serialize, TS)]
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

#[derive(Clone, Copy, Serialize, TS)]
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
    /// A local shell exited, with its exit code, or the signal that ended it.
    ProcessEnded { code: Option<u32>, signal: Option<String> },
    /// The connection broke (or the device went away) after the session had started.
    Lost(Error),
    /// Connecting, authenticating or starting the session failed.
    Failed(Error),
}

/// Pause reading once this many output bytes are unacknowledged...
const FLOW_HIGH: usize = 2 << 20;
/// ...and resume once the frontend has caught up to this many.
const FLOW_LOW: usize = 512 << 10;

/// Output flow control: counts the bytes sent to the frontend (or to a ZMODEM transfer) and
/// not yet acknowledged. Every backend stops reading while the count is too high, so the
/// output of a fast remote program (`cat` of a large file) waits on the remote side rather
/// than piling up in memory: in Tauri's IPC queue, which has no limit, and in xterm.js, which
/// drops writes past its own.
#[derive(Default)]
pub struct Flow {
    state: Mutex<FlowState>,
    /// Wakes reader threads ([`Flow::wait_ready`])...
    resumed: Condvar,
    /// ...and reading tasks ([`Flow::ready`]).
    resumed_async: tokio::sync::Notify,
}

#[derive(Default)]
struct FlowState {
    unacked: usize,
    /// Over [`FLOW_HIGH`] since it was last at most [`FLOW_LOW`].
    paused: bool,
    closed: bool,
}

impl FlowState {
    fn waits(&self) -> bool {
        self.paused && !self.closed
    }
}

impl Flow {
    pub fn sent(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.unacked = state.unacked.saturating_add(bytes);
        if state.unacked >= FLOW_HIGH {
            state.paused = true;
        }
    }

    pub fn ack(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        state.unacked = state.unacked.saturating_sub(bytes);
        if state.paused && state.unacked <= FLOW_LOW {
            state.paused = false;
            self.resumed.notify_all();
            self.resumed_async.notify_waiters();
        }
    }

    /// Whether a reader should wait before reading more.
    pub fn is_paused(&self) -> bool {
        self.state.lock().unwrap().waits()
    }

    /// Blocks while the frontend is too far behind, for backends reading on their own
    /// thread; returns at once when the session is closing.
    pub fn wait_ready(&self) {
        let _state = self.resumed.wait_while(self.state.lock().unwrap(), |state| state.waits()).unwrap();
    }

    /// [`Flow::wait_ready`] for backends reading in their task.
    pub async fn ready(&self) {
        loop {
            let resumed = self.resumed_async.notified();
            tokio::pin!(resumed);
            // Registered before checking, so that a resume in between is not missed.
            resumed.as_mut().enable();
            if !self.is_paused() {
                return;
            }
            resumed.await;
        }
    }

    /// Stops all waiting, for good (the session is closing).
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.resumed.notify_all();
        self.resumed_async.notify_waiters();
    }
}

type Release = Box<dyn FnOnce() + Send>;

/// What a session holds beyond its task (its SSH connection's registration, a waiting ZMODEM
/// transfer, the flow control its reader may wait on), as the releases to run when it closes.
///
/// A task or thread a backend starts besides its own either stops here (registered with
/// [`TermIo::on_close`]: a Telnet connection's writer) or ends by itself once the session's
/// side of it is dropped (a PTY's or serial port's reader and writer, whose input channel
/// closes and whose reads time out); otherwise it could outlive the session, holding its
/// connection open.
#[derive(Clone)]
pub struct Lifetime(Arc<Mutex<Option<Vec<Release>>>>);

impl Lifetime {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Some(Vec::new()))))
    }

    /// Runs `release` when the session closes; right away if it has already. Something the
    /// task registers just as the session closes is thereby released all the same.
    pub fn on_close(&self, release: impl FnOnce() + Send + 'static) {
        let mut releases = self.0.lock().unwrap();
        match releases.as_mut() {
            Some(releases) => releases.push(Box::new(release)),
            None => {
                drop(releases);
                release();
            }
        }
    }

    /// Runs the releases, once.
    fn close(&self) {
        let releases = self.0.lock().unwrap().take();
        for release in releases.into_iter().flatten() {
            release();
        }
    }
}

/// The first byte of a message on a session's channel: terminal output follows...
const OUTPUT: u8 = 0;
/// ...or a [`SessionEvent`] as JSON. One channel for both keeps them in order: the message
/// about how a session ended comes after its last output.
const EVENT: u8 = 1;

/// Output side of a session: terminal bytes and lifecycle events. Cloneable, so features
/// running beside the shell (port forwarding) can report to the same tab.
#[derive(Clone)]
pub struct SessionSink {
    channel: Channel,
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

    /// Ends the decoded output stream (see [`Codec::flush`]).
    pub fn flush_text(&self) {
        if let Some(codec) = &self.codec {
            let rest = codec.flush();
            if !rest.is_empty() {
                self.write(rest);
            }
        }
    }

    /// Remote output for the terminal, past ZMODEM detection: converted to UTF-8 first.
    pub fn remote(&self, bytes: Vec<u8>) {
        match &self.codec {
            Some(codec) => self.write(codec.decode(&bytes)),
            None => self.write(bytes),
        }
    }

    #[cfg(test)]
    pub fn zmodem(&self) -> &Arc<Zmodem> {
        &self.zmodem
    }

    /// The remote side's character encoding (for file names in ZMODEM transfers).
    pub fn encoding(&self) -> &'static encoding_rs::Encoding {
        self.codec.as_ref().map_or(encoding_rs::UTF_8, |codec| codec.encoding())
    }

    /// Writes to the terminal directly (our own messages, or output already filtered), and
    /// to the session's log.
    pub fn write(&self, bytes: Vec<u8>) {
        if let Some(error) = self.log.write(&bytes) {
            self.event(SessionEvent::Log { path: None, error: Some(error) });
        }
        self.send_output(&bytes);
    }

    fn send_output(&self, bytes: &[u8]) {
        self.flow.sent(bytes.len());
        let mut message = Vec::with_capacity(bytes.len() + 1);
        message.push(OUTPUT);
        message.extend_from_slice(bytes);
        // A send error means the frontend is gone; the session will be closed shortly.
        let _ = self.channel.send(InvokeResponseBody::Raw(message));
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
        self.send_output(RESET);
    }

    pub fn log(&self) -> &LogSlot {
        &self.log
    }

    /// Writes text, translating bare `\n` to `\r\n`.
    pub fn print(&self, text: &str) {
        self.write(text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes());
    }

    pub fn event(&self, event: SessionEvent) {
        let mut message = vec![EVENT];
        if serde_json::to_writer(&mut message, &event).is_ok() {
            let _ = self.channel.send(InvokeResponseBody::Raw(message));
        }
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
    lifetime: Lifetime,
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
        channel: Channel,
        size: (u16, u16),
        log: Arc<LogSlot>,
        encoding: &'static encoding_rs::Encoding,
    ) -> (Self, mpsc::UnboundedSender<SessionInput>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (zmodem, zmodem_out) = Zmodem::new();
        let codec = Codec::new(encoding).map(Arc::new);
        let sink = SessionSink { channel, flow: Arc::default(), zmodem, log, codec };
        let lifetime = Lifetime::new();
        // A reader waiting for the frontend would wait for ever, and a transfer waiting for
        // the user to choose files too.
        let (flow, zmodem) = (sink.flow.clone(), sink.zmodem.clone());
        lifetime.on_close(move || {
            flow.close();
            zmodem.cancel();
        });
        let io = Self { sink, lifetime, foreground: Foreground::default(), input: rx, zmodem_out, transfer_data: false, size };
        (io, tx)
    }

    /// A session without a frontend, for backend tests: returns the input sender and the
    /// channels' receiving ends (raw output bytes, events as JSON).
    #[cfg(test)]
    pub fn detached(
        size: (u16, u16),
    ) -> (Self, mpsc::UnboundedSender<SessionInput>, std::sync::mpsc::Receiver<Vec<u8>>, std::sync::mpsc::Receiver<String>) {
        Self::detached_with(size, encoding_rs::UTF_8)
    }

    /// [`TermIo::detached`] for a session in another character encoding.
    #[cfg(test)]
    pub fn detached_with(
        size: (u16, u16),
        encoding: &'static encoding_rs::Encoding,
    ) -> (Self, mpsc::UnboundedSender<SessionInput>, std::sync::mpsc::Receiver<Vec<u8>>, std::sync::mpsc::Receiver<String>) {
        let (output_tx, output_rx) = std::sync::mpsc::channel();
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(message) = body {
                match message.split_first() {
                    Some((&OUTPUT, bytes)) => drop(output_tx.send(bytes.to_vec())),
                    Some((&EVENT, json)) => drop(events_tx.send(String::from_utf8_lossy(json).into_owned())),
                    _ => {}
                }
            }
            Ok(())
        });
        let log = LogSlot::new(crate::logging::LogInfo::default());
        let (io, input) = Self::new(channel, size, log, encoding);
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

    /// Runs `release` when the session closes (see [`Lifetime::on_close`]).
    pub fn on_close(&self, release: impl FnOnce() + Send + 'static) {
        self.lifetime.on_close(release);
    }

    /// Reports how a remote session ended, in the terminal and as its `Closed` event. What
    /// the session held is released first: the tab may reconnect as soon as it hears.
    pub fn finish(&self, outcome: Outcome) {
        self.lifetime.close();
        self.sink.flush_text();
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
            Outcome::ProcessEnded { code, signal } => {
                let message = match (signal, code) {
                    (Some(signal), _) => t!("terminal.processSignaled", signal = signal),
                    (None, Some(code)) if code != 0 => t!("terminal.processExitedWithCode", code = code),
                    _ => t!("terminal.processExited"),
                };
                self.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
                (CloseReason::Exited, None, code)
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
        self.next_input(true, true).await
    }

    /// [`TermIo::recv`] without taking more of a ZMODEM transfer's data, which waits for the
    /// backend meanwhile: for a backend whose own queue is full, to still see resizes and
    /// keystrokes (which may cancel the transfer).
    pub async fn recv_typed(&mut self) -> Option<SessionInput> {
        self.next_input(true, false).await
    }

    /// `convert`: whether keystrokes are converted to the session's encoding. Prompts read
    /// UTF-8, which is also what the SSH protocol uses for passwords and answers.
    /// `transfers`: whether to take a ZMODEM transfer's data.
    async fn next_input(&mut self, convert: bool, transfers: bool) -> Option<SessionInput> {
        loop {
            self.transfer_data = false;
            let input = tokio::select! {
                biased;
                Some(data) = self.zmodem_out.recv(), if transfers => {
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
            let SessionInput::Data(data) = self.next_input(false, true).await? else {
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

impl Drop for TermIo {
    fn drop(&mut self) {
        self.lifetime.close();
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
    /// The webview whose tab shows the session.
    webview: String,
    input: mpsc::UnboundedSender<SessionInput>,
    sink: SessionSink,
    lifetime: Lifetime,
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
    /// Starts a session whose backend task is produced by `backend`, for a tab of the webview
    /// labelled `webview`, logging to `log` (which may already be writing, so that the log
    /// starts with the session's first output). `encoding` is the remote side's character
    /// encoding.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn<F, Fut>(
        &self,
        webview: &str,
        channel: Channel,
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
        let (io, input) = TermIo::new(channel, size, log, encoding);
        let sink = io.sink();
        let lifetime = io.lifetime.clone();
        let flow = io.sink.flow();
        let foreground = io.foreground();
        let zmodem = io.sink.zmodem.clone();
        let task = tauri::async_runtime::spawn(backend(id, io));
        let entry = SessionEntry { webview: webview.to_owned(), input, sink, lifetime, flow, foreground, zmodem, task };
        self.sessions.lock().unwrap().insert(id, entry);
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

    /// Closes a session: releases what it holds, then aborts its task.
    pub fn remove(&self, id: SessionId) -> Result<()> {
        let entry = self.sessions.lock().unwrap().remove(&id).ok_or_else(|| Error::new("session.notFound"))?;
        entry.lifetime.close();
        entry.task.abort();
        Ok(())
    }

    /// Closes the sessions of a webview whose page is going away (reloading, or its window
    /// closing): nothing would show them or close them any more.
    pub fn remove_webview(&self, webview: &str) {
        let ids: Vec<SessionId> =
            self.sessions.lock().unwrap().iter().filter(|(_, entry)| entry.webview == webview).map(|(id, _)| *id).collect();
        for id in ids {
            let _ = self.remove(id);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading pauses once too much is unacknowledged and resumes only once the frontend has
    /// caught up most of the way, for threads and tasks alike.
    #[tokio::test(flavor = "multi_thread")]
    async fn flow_pauses_and_resumes() {
        let flow = Arc::new(Flow::default());
        flow.sent(FLOW_HIGH - 1);
        assert!(!flow.is_paused());
        flow.sent(1);
        assert!(flow.is_paused());
        let waiting = tokio::spawn({
            let flow = flow.clone();
            async move { flow.ready().await }
        });
        let thread = std::thread::spawn({
            let flow = flow.clone();
            move || flow.wait_ready()
        });
        // Below the high mark, but not yet at the low one.
        flow.ack(FLOW_HIGH - FLOW_LOW - 1);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(flow.is_paused() && !waiting.is_finished() && !thread.is_finished());
        flow.ack(1);
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting).await.unwrap().unwrap();
        thread.join().unwrap();

        // Closing the session wakes waiters too.
        flow.sent(FLOW_HIGH);
        let waiting = tokio::spawn({
            let flow = flow.clone();
            async move { flow.ready().await }
        });
        flow.close();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting).await.unwrap().unwrap();
    }

    /// Counts how often something was released.
    fn counter() -> (Arc<std::sync::atomic::AtomicUsize>, impl Fn() -> usize) {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let read = count.clone();
        (count, move || read.load(Ordering::SeqCst))
    }

    /// Starts a session for `webview` whose backend holds a release and runs until aborted;
    /// returns how often its release ran, and whether its task has been dropped.
    fn hold(sessions: &SessionManager, webview: &str) -> (SessionId, impl Fn() -> usize, impl Fn() -> usize) {
        let (released, released_count) = counter();
        let (dropped, dropped_count) = counter();
        let id = sessions.spawn(webview, Channel::new(|_| Ok(())), (80, 24), LogSlot::new(Default::default()), encoding_rs::UTF_8, move |_, io| async move {
            io.on_close(move || {
                released.fetch_add(1, Ordering::SeqCst);
            });
            struct Dropped(Arc<std::sync::atomic::AtomicUsize>);
            impl Drop for Dropped {
                fn drop(&mut self) {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
            let _dropped = Dropped(dropped);
            let _io = io;
            std::future::pending::<()>().await;
        });
        (id, released_count, dropped_count)
    }

    async fn eventually(done: impl Fn() -> bool) {
        for _ in 0..100 {
            if done() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("timed out");
    }

    /// A page that reloads (or a window that closes) takes its sessions with it, and only its.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_page_reloading_closes_its_sessions() {
        let sessions = SessionManager::default();
        let (_, a_released, a_dropped) = hold(&sessions, "main");
        let (_, b_released, b_dropped) = hold(&sessions, "main");
        let (other, other_released, _) = hold(&sessions, "other");
        // The tasks register their releases once running.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        sessions.remove_webview("main");
        assert_eq!(sessions.len(), 1);
        // Released right away, before the aborted tasks are dropped.
        assert_eq!((a_released(), b_released(), other_released()), (1, 1, 0));
        eventually(|| a_dropped() == 1 && b_dropped() == 1).await;
        // Dropping the task's terminal side doesn't release twice.
        assert_eq!((a_released(), b_released()), (1, 1));

        sessions.remove(other).unwrap();
        assert_eq!((sessions.len(), other_released()), (0, 1));
    }

    /// Something registered once the session has closed (a task that connected just as its
    /// tab closed) is released at once.
    #[test]
    fn a_closed_lifetime_releases_at_once() {
        let lifetime = Lifetime::new();
        let (released, count) = counter();
        let early = released.clone();
        lifetime.on_close(move || {
            early.fetch_add(1, Ordering::SeqCst);
        });
        lifetime.close();
        lifetime.close();
        assert_eq!(count(), 1);
        lifetime.on_close(move || {
            released.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(count(), 2);
    }

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

    #[test]
    fn an_incomplete_character_shows_when_the_session_ends() {
        let (io, _input, output, _events) = TermIo::detached_with((80, 24), encoding_rs::GBK);
        io.output(b"ok \xc4".to_vec());
        io.finish(Outcome::Exited(Some(0)));
        let shown = String::from_utf8(output.try_iter().flatten().collect()).unwrap();
        assert!(shown.starts_with("ok \u{fffd}"), "{shown:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_incomplete_character_does_not_reach_past_a_transfer() {
        let (io, _input, output, _events) = TermIo::detached_with((80, 24), encoding_rs::GBK);
        let sink = io.sink();
        sink.output(b"ok \xc4".to_vec());
        sink.output(b"**\x18B00000000000000\r\x8a\x11".to_vec());
        assert!(sink.zmodem.is_active());
        let shown = String::from_utf8(output.try_iter().flatten().collect()).unwrap();
        assert!(shown.starts_with("ok \u{fffd}"), "{shown:?}");
        sink.zmodem.cancel();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_header_split_across_reads_does_not_show() {
        let (io, _input, output, events) = TermIo::detached((80, 24));
        let sink = io.sink();
        for chunk in [b"rz\r**\x18".as_slice(), b"B", b"0", b"0000000000000\r\x8a\x11"] {
            sink.output(chunk.to_vec());
        }
        let asked = events.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert!(asked.contains("chooseDestination"), "{asked}");
        let shown: Vec<u8> = output.try_iter().flatten().collect();
        assert_eq!(String::from_utf8_lossy(&shown), "rz\r\r\n");
        sink.zmodem.cancel();
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
