//! The output side of a session.

use std::sync::{Arc, Mutex};

use tauri::ipc::{Channel, InvokeResponseBody};

use super::{Flow, SessionEvent};
use crate::encoding::Codec;
use crate::logging::LogSlot;
use crate::zmodem::Zmodem;

/// The first byte of a message on a session's channel: terminal output follows...
pub(super) const OUTPUT: u8 = 0;
/// ...or a [`SessionEvent`] as JSON. One channel for both keeps them in order: the message
/// about how a session ended comes after its last output.
pub(super) const EVENT: u8 = 1;

/// A session's channel message for an event.
fn event_message(event: &SessionEvent) -> Option<InvokeResponseBody> {
    let mut message = vec![EVENT];
    serde_json::to_writer(&mut message, event).ok()?;
    Some(InvokeResponseBody::Raw(message))
}

/// Output side of a session: terminal bytes and lifecycle events. Cloneable, so features
/// running beside the shell (port forwarding) can report to the same tab.
#[derive(Clone)]
pub struct SessionSink {
    /// To the tab showing the session; replaced when it moves to another window
    /// ([`SessionSink::move_to`]), for every clone.
    channel: Arc<Mutex<Channel>>,
    flow: Arc<Flow>,
    pub(super) zmodem: Arc<Zmodem>,
    log: Arc<LogSlot>,
    /// The session's character encoding; `None` for UTF-8.
    pub(super) codec: Option<Arc<Codec>>,
}

impl SessionSink {
    pub(super) fn new(channel: Channel, zmodem: Arc<Zmodem>, log: Arc<LogSlot>, codec: Option<Arc<Codec>>) -> Self {
        Self { channel: Arc::new(Mutex::new(channel)), flow: Arc::default(), zmodem, log, codec }
    }

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
        let mut message = Vec::with_capacity(bytes.len() + 1);
        message.push(OUTPUT);
        message.extend_from_slice(bytes);
        // Counted and sent under the lock, so that a move falls before both or after both.
        let channel = self.channel.lock().unwrap();
        self.flow.sent(bytes.len());
        // A send error means the frontend is gone; the session will be closed shortly.
        let _ = channel.send(InvokeResponseBody::Raw(message));
    }

    /// Sends what follows to `channel`, another window's tab: the old channel's last message
    /// is [`SessionEvent::Moved`], after all the output it was sent. What the old tab hasn't
    /// acknowledged no longer counts.
    pub(super) fn move_to(&self, channel: Channel, move_id: String) {
        let mut current = self.channel.lock().unwrap();
        if let Some(message) = event_message(&SessionEvent::Moved { move_id }) {
            let _ = current.send(message);
        }
        *current = channel;
        self.flow.reset();
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
        if let Some(message) = event_message(&event) {
            let _ = self.channel.lock().unwrap().send(message);
        }
    }
}
