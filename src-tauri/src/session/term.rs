//! The terminal side of a session, as its backend sees it.

use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use super::{CloseReason, Lifetime, Outcome, SessionEvent, SessionInput, SessionSink};
use crate::encoding::Codec;
use crate::logging::LogSlot;
use crate::zmodem::Zmodem;

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
    pub(super) sink: SessionSink,
    pub(super) lifetime: Lifetime,
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
    pub(super) fn new(
        channel: Channel,
        size: (u16, u16),
        log: Arc<LogSlot>,
        encoding: &'static encoding_rs::Encoding,
    ) -> (Self, mpsc::UnboundedSender<SessionInput>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (zmodem, zmodem_out) = Zmodem::new();
        let codec = Codec::new(encoding).map(Arc::new);
        let sink = SessionSink::new(channel, zmodem, log, codec);
        let lifetime = Lifetime::new();
        // A reader waiting for the frontend would wait for ever, and a transfer waiting for
        // the user to choose files too.
        let (flow, zmodem) = (sink.flow(), sink.zmodem.clone());
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
        use super::sink::{EVENT, OUTPUT};
        use tauri::ipc::InvokeResponseBody;

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
        let (reason, error, status, signal) = match outcome {
            Outcome::Exited(status) => {
                let message = match status {
                    Some(status) => t!("terminal.closedWithStatus", status = status),
                    None => t!("terminal.closed"),
                };
                self.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
                (CloseReason::Exited, None, status, None)
            }
            Outcome::Signaled(signal) => {
                let message = t!("terminal.closedBySignal", signal = signal);
                self.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
                (CloseReason::Exited, None, None, Some(signal))
            }
            Outcome::ProcessEnded { code, signal } => {
                let message = match (&signal, code) {
                    (Some(signal), _) => t!("terminal.processSignaled", signal = signal),
                    (None, Some(code)) if code != 0 => t!("terminal.processExitedWithCode", code = code),
                    _ => t!("terminal.processExited"),
                };
                self.print(&format!("\n\x1b[2m{message}\x1b[0m\n"));
                (CloseReason::Exited, None, code, signal)
            }
            Outcome::Lost(e) => {
                // The remote program may have left the cursor mid-line.
                self.print(&format!("\n\x1b[31m{e}\x1b[0m\n"));
                (CloseReason::Lost, Some(e), None, None)
            }
            Outcome::Failed(e) => {
                self.print(&format!("\x1b[31m{e}\x1b[0m\n"));
                (CloseReason::Failed, Some(e), None, None)
            }
        };
        self.event(SessionEvent::Closed { reason, error, status, signal });
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
            // Typing first: a transfer whose data is always ready (streaming to a receiver
            // that doesn't acknowledge each subpacket, over a slower link) would otherwise
            // keep Ctrl+C from ever being read. Typing during a transfer is taken here, so
            // it can't hold the transfer up.
            let input = tokio::select! {
                biased;
                input = self.input.recv() => input,
                Some(data) = self.zmodem_out.recv(), if transfers => {
                    self.transfer_data = true;
                    return Some(SessionInput::Data(data));
                }
            };
            match &input {
                Some(SessionInput::Data(data)) if self.sink.zmodem.is_active() => {
                    self.sink.zmodem.input(data);
                    continue;
                }
                Some(SessionInput::Raw(_)) if self.sink.zmodem.is_active() => continue,
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

    /// Typing is converted to the session's encoding; the terminal's binary reports are not.
    #[tokio::test]
    async fn raw_input_is_passed_on_as_it_is() {
        let (mut io, input, _output, _events) = TermIo::detached_with((80, 24), encoding_rs::GBK);
        input.send(SessionInput::Data("中".as_bytes().to_vec())).unwrap();
        // A click at column 129 in the X10 mouse encoding.
        input.send(SessionInput::Raw(b"\x1b[M \xa1!".to_vec())).unwrap();
        assert!(matches!(io.recv().await, Some(SessionInput::Data(data)) if data == [0xd6, 0xd0]));
        assert!(matches!(io.recv().await, Some(SessionInput::Raw(data)) if data == b"\x1b[M \xa1!"));
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
