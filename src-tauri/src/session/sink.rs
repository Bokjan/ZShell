//! The output side of a session.

use std::sync::{Arc, Mutex};

use chrono::Local;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::notice::{self, Tone};
use super::{Flow, SessionEvent, SessionId};
use crate::encoding::Codec;
use crate::logging::LogSlot;
use crate::traffic::{ChannelInfo, Meter, Reporter, Wake};
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

/// The tabs that something beside their sessions reports to (the counts of an SFTP channel,
/// which every tab on its connection shows).
#[derive(Clone, Default)]
pub struct Viewers(Arc<Mutex<Vec<(SessionId, SessionSink)>>>);

impl Viewers {
    pub fn add(&self, id: SessionId, sink: SessionSink) {
        self.0.lock().unwrap().push((id, sink));
    }

    pub fn remove(&self, id: SessionId) {
        self.0.lock().unwrap().retain(|(viewer, _)| *viewer != id);
    }

    /// Sends `event` to each of them, put into a message once.
    pub fn send(&self, event: &SessionEvent) {
        let Some(message) = event_message(event) else { return };
        for (_, sink) in self.0.lock().unwrap().iter() {
            let _ = sink.channel.lock().unwrap().send(message.clone());
        }
    }
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
    traffic: Arc<TerminalTraffic>,
}

/// What a session's terminal sends and receives, as the remote side has it (before the
/// session's encoding is converted, ZMODEM transfers included, our own messages and what is
/// typed at prompts before the session starts not), reported to its tab.
pub struct TerminalTraffic {
    meter: Arc<Meter>,
    channel: Arc<Mutex<Option<ChannelInfo>>>,
    wake: Wake,
    _reporter: Reporter,
}

impl TerminalTraffic {
    /// Reported on `to`, the session's channel.
    fn new(to: Arc<Mutex<Channel>>) -> Self {
        let wake = Wake::default();
        let meter = Arc::new(Meter::new(&wake));
        let channel = Arc::new(Mutex::new(None));
        let (counted, on) = (meter.clone(), channel.clone());
        let reporter = Reporter::start(&wake, move || {
            let event = SessionEvent::TerminalTraffic { counts: counted.counts(), channel: *on.lock().unwrap() };
            if let Some(message) = event_message(&event) {
                let _ = to.lock().unwrap().send(message);
            }
        });
        Self { meter, channel, wake, _reporter: reporter }
    }

    pub fn sent(&self, bytes: usize) {
        self.meter.sent(bytes);
    }

    #[cfg(test)]
    pub fn counts(&self) -> crate::traffic::Counts {
        self.meter.counts()
    }

    /// The SSH channel the session's shell runs on.
    pub fn on_channel(&self, channel: ChannelInfo) {
        *self.channel.lock().unwrap() = Some(channel);
        self.wake.wake();
    }
}

impl SessionSink {
    pub(super) fn new(channel: Channel, zmodem: Arc<Zmodem>, log: Arc<LogSlot>, codec: Option<Arc<Codec>>) -> Self {
        let channel = Arc::new(Mutex::new(channel));
        let traffic = Arc::new(TerminalTraffic::new(channel.clone()));
        Self { channel, flow: Arc::default(), zmodem, log, codec, traffic }
    }

    pub fn traffic(&self) -> &TerminalTraffic {
        &self.traffic
    }

    /// Output from the remote side (or the local shell): shown in the terminal, unless a
    /// ZMODEM transfer starts or is running.
    pub fn output(&self, bytes: Vec<u8>) {
        self.traffic.meter.received(bytes.len());
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
        self.log_output(&bytes);
        self.send_output(&bytes);
    }

    fn log_output(&self, bytes: &[u8]) {
        if let Some(error) = self.log.write(bytes) {
            self.event(SessionEvent::Log { path: None, error: Some(error) });
        }
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

    /// Writes text as it is, translating bare `\n` to `\r\n`: the login dialogue, echoed
    /// input and other programs' output. Our own messages are [`SessionSink::notice`]s.
    pub fn print(&self, text: &str) {
        self.write(text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes());
    }

    /// One of our own messages, from where the cursor is (the start of a line), each line
    /// starting with the time (see [`notice`]).
    pub fn notice(&self, tone: Tone, text: &str) {
        self.write_notice(tone, text, true);
    }

    /// A progress line, replacing the line the cursor is on and left open: the next one, or
    /// the message it ends with, replaces it.
    pub fn notice_progress(&self, tone: Tone, text: &str) {
        self.write(b"\r\x1b[K".to_vec());
        self.write_notice(tone, text, false);
    }

    fn write_notice(&self, tone: Tone, text: &str, end_line: bool) {
        let time = crate::logging::time_format::current().format(Local::now().naive_local());
        let notice = notice::render(tone, text, &time, end_line);
        // Not twice in a log that starts its lines with the time itself.
        self.log_output(if self.log.stamps_lines() { &notice.unstamped } else { &notice.shown }.as_bytes());
        self.send_output(notice.shown.as_bytes());
    }

    pub fn event(&self, event: SessionEvent) {
        if let Some(message) = event_message(&event) {
            let _ = self.channel.lock().unwrap().send(message);
        }
    }
}
