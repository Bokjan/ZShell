//! Terminal sessions.
//!
//! A session is a background task that owns a [`TermIo`]: it streams output bytes to the
//! frontend through a [`tauri::ipc::Channel`], reports lifecycle [`SessionEvent`]s, and
//! consumes [`SessionInput`] (keystrokes, resizes) from the frontend. Closing a session
//! aborts its task.
//!
//! Port forwards report their state through the same event channel ([`SessionSink`]).
//!
//! SSH shells are the only backend for now; local PTYs (M5) will plug in the same way.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use crate::error::{Error, Result};
use crate::forward::ForwardState;

pub type SessionId = u32;

#[derive(Debug)]
pub enum SessionInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    Connected,
    Closed { reason: CloseReason, error: Option<Error> },
    Forward { rule_id: String, state: ForwardState },
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseReason {
    /// The remote shell exited or closed the session.
    Exited,
    /// The connection broke after the session had started.
    Lost,
    /// Connecting, authenticating or starting the shell failed.
    Failed,
}

/// Output side of a session: terminal bytes and lifecycle events. Cloneable, so features
/// running beside the shell (port forwarding) can report to the same tab.
#[derive(Clone)]
pub struct SessionSink {
    output: Channel,
    events: Channel<SessionEvent>,
}

impl SessionSink {
    pub fn write(&self, bytes: Vec<u8>) {
        // A send error means the frontend is gone; the session will be closed shortly.
        let _ = self.output.send(InvokeResponseBody::Raw(bytes));
    }

    /// Writes text, translating bare `\n` to `\r\n`.
    pub fn print(&self, text: &str) {
        self.write(text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes());
    }

    pub fn event(&self, event: SessionEvent) {
        let _ = self.events.send(event);
    }
}

/// The terminal side of a session, as seen by its backend task.
pub struct TermIo {
    sink: SessionSink,
    input: mpsc::UnboundedReceiver<SessionInput>,
    /// Latest terminal size (cols, rows) reported by the frontend.
    pub size: (u16, u16),
}

impl TermIo {
    pub fn write(&self, bytes: Vec<u8>) {
        self.sink.write(bytes);
    }

    pub fn print(&self, text: &str) {
        self.sink.print(text);
    }

    pub fn event(&self, event: SessionEvent) {
        self.sink.event(event);
    }

    pub fn sink(&self) -> SessionSink {
        self.sink.clone()
    }

    /// Receives the next input, keeping [`TermIo::size`] up to date.
    pub async fn recv(&mut self) -> Option<SessionInput> {
        let input = self.input.recv().await;
        if let Some(SessionInput::Resize { cols, rows }) = input {
            self.size = (cols, rows);
        }
        input
    }

    /// Reads one line typed into the terminal, for prompts shown before the remote shell
    /// starts. Returns `None` if the user cancels with Ctrl+C / Ctrl+D or the session closes.
    pub async fn read_line(&mut self, echo: bool) -> Option<String> {
        let mut line = String::new();
        loop {
            let SessionInput::Data(data) = self.recv().await? else {
                continue;
            };
            // Escape sequences (arrow keys etc.) are not supported in prompts.
            if data.first() == Some(&0x1b) {
                continue;
            }
            for c in String::from_utf8_lossy(&data).chars() {
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

struct SessionEntry {
    input: mpsc::UnboundedSender<SessionInput>,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub struct SessionManager {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<SessionId, SessionEntry>>,
}

impl SessionManager {
    /// Starts a session whose backend task is produced by `backend`.
    pub fn spawn<F, Fut>(
        &self,
        output: Channel,
        events: Channel<SessionEvent>,
        size: (u16, u16),
        backend: F,
    ) -> SessionId
    where
        F: FnOnce(SessionId, TermIo) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::unbounded_channel();
        let io = TermIo { sink: SessionSink { output, events }, input: rx, size };
        let task = tauri::async_runtime::spawn(backend(id, io));
        self.sessions.lock().unwrap().insert(id, SessionEntry { input: tx, task });
        id
    }

    pub fn send(&self, id: SessionId, input: SessionInput) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        let entry = sessions.get(&id).ok_or_else(|| Error::new("session.notFound"))?;
        // A send error means the backend already exited; it will be removed on close.
        let _ = entry.input.send(input);
        Ok(())
    }

    pub fn remove(&self, id: SessionId) -> Result<()> {
        let entry = self.sessions.lock().unwrap().remove(&id).ok_or_else(|| Error::new("session.notFound"))?;
        entry.task.abort();
        Ok(())
    }
}
