//! The byte stream between a transfer and the other side: remote output arrives in chunks
//! (redirected from the terminal), and what the transfer sends goes out through a bounded
//! channel to the session backend, so a fast sender waits for the connection.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use anyhow::{bail, Result};
use tokio::sync::{mpsc, watch};

use super::frame::{self, Encoding, Escaped, Header, CAN, ZDLE, ZPAD};
use crate::error::Error;

/// How long to wait for the other side before retrying (as lrzsz's default of 10 s).
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Five CANs in a row cancel a transfer.
const CANCEL_CANS: u8 = 5;
/// Bytes of non-header data tolerated while waiting for a header.
const GARBAGE_LIMIT: usize = 1 << 20;
/// The largest data subpacket accepted (lrzsz sends up to 8 KiB).
const MAX_SUBPACKET: usize = 8192;
/// Outgoing data is buffered up to this size before it is handed to the connection.
const SEND_BATCH: usize = 32 * 1024;

/// Whether `e` is a read that timed out, after which the protocol asks again.
pub fn is_timeout(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Error>().is_some_and(|e| e.code() == "zmodem.timeout")
}

/// Why reading a subpacket failed in a way the protocol recovers from (by asking for the data
/// again with `ZRPOS`).
pub enum Bad {
    /// A bad CRC or escape, or too long.
    Corrupt,
    /// Nothing arrived for a while (a slow jump host, a key re-exchange, a Wi-Fi hiccup).
    Timeout,
}

pub struct Link {
    incoming: mpsc::UnboundedReceiver<Vec<u8>>,
    buffer: VecDeque<u8>,
    outgoing: mpsc::Sender<Vec<u8>>,
    pending: Vec<u8>,
    cancel: watch::Receiver<bool>,
    /// CANs read in a row.
    cans: u8,
    /// The encoding of the last header read; binary ones set the CRC of their subpackets.
    pub last_encoding: Encoding,
    /// The remote side's character encoding, for file names.
    pub charset: &'static encoding_rs::Encoding,
    /// How long to wait for the other side before asking again: [`TIMEOUT`], shorter in tests.
    pub timeout: Duration,
    /// What has been read since [`Link::record`], and how many of those bytes the terminal
    /// already showed.
    recording: Option<(Vec<u8>, usize)>,
}

impl Link {
    pub fn new(
        incoming: mpsc::UnboundedReceiver<Vec<u8>>,
        outgoing: mpsc::Sender<Vec<u8>>,
        cancel: watch::Receiver<bool>,
    ) -> Self {
        Self {
            incoming,
            buffer: VecDeque::new(),
            outgoing,
            pending: Vec::new(),
            cancel,
            cans: 0,
            last_encoding: Encoding::Hex,
            charset: encoding_rs::UTF_8,
            timeout: TIMEOUT,
            recording: None,
        }
    }

    /// Keeps what is read from now on, to give it back through [`Link::take_rest`] if it turns
    /// out not to be a transfer. The first `shown` bytes were shown in the terminal already.
    pub fn record(&mut self, shown: usize) {
        self.recording = Some((Vec::new(), shown));
    }

    pub fn stop_recording(&mut self) {
        self.recording = None;
    }

    fn pop(&mut self) -> Option<u8> {
        let byte = self.buffer.pop_front()?;
        if let Some((recorded, _)) = &mut self.recording {
            recorded.push(byte);
        }
        Some(byte)
    }

    /// Fails with `zmodem.cancelled` once the user cancels.
    fn check_cancelled(&self) -> Result<()> {
        if *self.cancel.borrow() {
            bail!(Error::new("zmodem.cancelled"));
        }
        Ok(())
    }

    /// Waits for more input, for at most `timeout`.
    async fn fill(&mut self, timeout: Duration) -> Result<()> {
        self.check_cancelled()?;
        tokio::select! {
            _ = self.cancel.wait_for(|cancelled| *cancelled) => bail!(Error::new("zmodem.cancelled")),
            // The session has ended (its tab closed while a question waits for the user, say):
            // nothing will arrive, and nothing we send would go anywhere.
            () = self.outgoing.closed() => bail!(Error::new("zmodem.cancelled")),
            chunk = self.incoming.recv() => match chunk {
                Some(chunk) => self.buffer.extend(chunk),
                // The session is closing.
                None => bail!(Error::new("zmodem.cancelled")),
            },
            _ = tokio::time::sleep(timeout) => bail!(Error::new("zmodem.timeout")),
        }
        Ok(())
    }

    /// Takes everything that has arrived without waiting.
    fn fill_ready(&mut self) {
        while let Ok(chunk) = self.incoming.try_recv() {
            self.buffer.extend(chunk);
        }
    }

    async fn byte(&mut self, timeout: Duration) -> Result<u8> {
        loop {
            if let Some(byte) = self.pop() {
                if byte == CAN {
                    self.cans += 1;
                    if self.cans >= CANCEL_CANS {
                        bail!(Error::new("zmodem.remoteCancelled"));
                    }
                } else {
                    self.cans = 0;
                }
                return Ok(byte);
            }
            self.fill(timeout).await?;
        }
    }

    /// Reads the next header, skipping anything before it.
    pub async fn header(&mut self, timeout: Duration) -> Result<Header> {
        let mut skipped = 0;
        loop {
            if skipped > GARBAGE_LIMIT {
                bail!(Error::new("zmodem.protocol"));
            }
            skipped += 1;
            if self.byte(timeout).await? != ZPAD {
                continue;
            }
            let mut byte = self.byte(timeout).await?;
            while byte == ZPAD {
                byte = self.byte(timeout).await?;
            }
            if byte != ZDLE {
                continue;
            }
            let Some(encoding) = frame::encoding_of(self.byte(timeout).await?) else {
                continue;
            };
            let header = match encoding {
                Encoding::Hex => self.hex_header(timeout).await?,
                _ => self.bin_header(encoding, timeout).await?,
            };
            if let Some(header) = header {
                self.last_encoding = encoding;
                return Ok(header);
            }
        }
    }

    async fn hex_header(&mut self, timeout: Duration) -> Result<Option<Header>> {
        let mut digits = [0u8; 14];
        for digit in &mut digits {
            *digit = self.byte(timeout).await?;
        }
        let header = frame::parse_hex_header(&digits);
        // CR LF follow; drop them so they aren't read as subpacket data.
        if self.peek(timeout).await? == b'\r' {
            self.pop();
            if self.peek(timeout).await? & 0x7f == b'\n' {
                self.pop();
            }
        }
        Ok(header)
    }

    async fn bin_header(&mut self, encoding: Encoding, timeout: Duration) -> Result<Option<Header>> {
        let mut raw = [0u8; 5];
        for byte in &mut raw {
            let Some(b) = self.header_byte(timeout).await? else { return Ok(None) };
            *byte = b;
        }
        let mut crc = [0u8; 4];
        let crc = &mut crc[..if encoding == Encoding::Bin32 { 4 } else { 2 }];
        for byte in crc.iter_mut() {
            let Some(b) = self.header_byte(timeout).await? else { return Ok(None) };
            *byte = b;
        }
        Ok(frame::parse_bin_header(raw, crc, encoding))
    }

    /// One byte of a binary header, skipping flow control characters (as subpackets and
    /// lrzsz do: a serial line with software flow control inserts them anywhere); `None` for
    /// anything but a data byte.
    async fn header_byte(&mut self, timeout: Duration) -> Result<Option<u8>> {
        loop {
            match self.escaped(timeout).await? {
                None => continue,
                Some(Escaped::Byte(b)) => return Ok(Some(b)),
                Some(_) => return Ok(None),
            }
        }
    }

    async fn peek(&mut self, timeout: Duration) -> Result<u8> {
        loop {
            if let Some(&byte) = self.buffer.front() {
                return Ok(byte);
            }
            self.fill(timeout).await?;
        }
    }

    /// Reads one byte of escaped data; `None` for flow control characters.
    async fn escaped(&mut self, timeout: Duration) -> Result<Option<Escaped>> {
        let byte = self.byte(timeout).await?;
        if byte != ZDLE {
            return Ok(if frame::is_flow_control(byte) { None } else { Some(Escaped::Byte(byte)) });
        }
        loop {
            let byte = self.byte(timeout).await?;
            if !frame::is_flow_control(byte) {
                return Ok(Some(frame::unescape(byte)));
            }
        }
    }

    /// Reads a data subpacket of the current frame: its data and how it ended. Damage and
    /// timeouts are an `Err` inside `Ok`, so the caller can ask for the data again.
    pub async fn subpacket(&mut self, data: &mut Vec<u8>) -> Result<std::result::Result<u8, Bad>> {
        match self.read_subpacket(data).await {
            Err(e) if is_timeout(&e) => Ok(Err(Bad::Timeout)),
            result => result,
        }
    }

    async fn read_subpacket(&mut self, data: &mut Vec<u8>) -> Result<std::result::Result<u8, Bad>> {
        data.clear();
        let crc32 = self.last_encoding == Encoding::Bin32;
        loop {
            match self.escaped(self.timeout).await? {
                None => {}
                Some(Escaped::Byte(byte)) => {
                    if data.len() >= MAX_SUBPACKET {
                        return Ok(Err(Bad::Corrupt));
                    }
                    data.push(byte);
                }
                Some(Escaped::Invalid) => return Ok(Err(Bad::Corrupt)),
                Some(Escaped::End(end)) => {
                    let mut crc = [0u8; 4];
                    let crc = &mut crc[..if crc32 { 4 } else { 2 }];
                    for byte in crc.iter_mut() {
                        loop {
                            match self.escaped(self.timeout).await? {
                                None => continue,
                                Some(Escaped::Byte(b)) => *byte = b,
                                _ => return Ok(Err(Bad::Corrupt)),
                            }
                            break;
                        }
                    }
                    let ok = frame::subpacket_crc_ok(data, end, crc, crc32);
                    return Ok(if ok { Ok(end) } else { Err(Bad::Corrupt) });
                }
            }
        }
    }

    /// While sending a file: a header that has arrived (the receiver asking to resume
    /// elsewhere, or cancelling), without waiting for one otherwise.
    pub async fn header_if_any(&mut self) -> Result<Option<Header>> {
        self.fill_ready();
        // Anything but a header (stray flow control) is dropped.
        while self.buffer.front().is_some_and(|&b| b != ZPAD && b != CAN) {
            self.buffer.pop_front();
        }
        if self.buffer.is_empty() {
            return Ok(None);
        }
        // The rest of the header is on its way.
        Ok(Some(self.header(Duration::from_secs(2)).await?))
    }

    /// Queues bytes to send; they go out once enough has collected, or at `flush`.
    pub async fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() >= SEND_BATCH {
            self.flush().await?;
        }
        Ok(())
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.check_cancelled()?;
        if self.pending.is_empty() {
            return Ok(());
        }
        let bytes = std::mem::take(&mut self.pending);
        tokio::select! {
            _ = self.cancel.wait_for(|cancelled| *cancelled) => bail!(Error::new("zmodem.cancelled")),
            sent = self.outgoing.send(bytes) => {
                if sent.is_err() {
                    bail!(Error::new("zmodem.cancelled"));
                }
            }
        }
        Ok(())
    }

    /// Sends a header right away.
    pub async fn send_header(&mut self, header: Header, encoding: Encoding) -> Result<()> {
        self.send(&frame::encode_header(&header, encoding)).await?;
        self.flush().await
    }

    /// Waits for `task` (a question to the user), meanwhile discarding what arrives (the
    /// other side repeating its init header) but noticing if it cancels or the user does.
    pub async fn wait_for<T>(&mut self, task: impl Future<Output = T>) -> Result<T> {
        tokio::pin!(task);
        loop {
            tokio::select! {
                value = &mut task => return Ok(value),
                read = self.byte(Duration::MAX) => { read?; }
            }
        }
    }

    /// Reads the `OO` that ends a session, if it comes, leaving anything else unread.
    pub async fn over_and_out(&mut self) {
        for _ in 0..2 {
            match self.peek(Duration::from_millis(500)).await {
                Ok(b'O') => {
                    self.buffer.pop_front();
                }
                _ => return,
            }
        }
    }

    /// Unread bytes, including those not yet taken from the channel (output that followed
    /// the transfer and belongs to the terminal), after what was recorded and not shown yet.
    pub fn take_rest(&mut self) -> Vec<u8> {
        self.fill_ready();
        let (mut rest, shown) = self.recording.take().unwrap_or_default();
        rest.extend(self.buffer.drain(..));
        rest.split_off(shown.min(rest.len()))
    }

    /// After a cancel, reads output until the other side has been quiet for `quiet` (at most
    /// `limit`) and returns what followed the last CAN: the data still in flight is not for
    /// the terminal, but the shell prompt after the program exits is. (Escaped ZMODEM data is
    /// full of ZDLEs, which are CANs; the program's own cancel ends with CANs too.)
    pub async fn drain(&mut self, quiet: Duration, limit: Duration) -> Vec<u8> {
        let mut kept: Vec<u8> = self.buffer.drain(..).collect();
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            if let Some(last) = kept.iter().rposition(|&b| b == CAN) {
                kept.drain(..=last);
            }
            let wait = quiet.min(deadline.saturating_duration_since(tokio::time::Instant::now()));
            match tokio::time::timeout(wait, self.incoming.recv()).await {
                Ok(Some(chunk)) if tokio::time::Instant::now() < deadline => kept.extend(chunk),
                _ => return kept,
            }
        }
    }

    /// Sends the abort sequence directly, even after the user has cancelled.
    pub async fn abort(&mut self) {
        self.pending.clear();
        self.send_now(frame::ABORT_SEQUENCE).await;
    }

    /// Sends `bytes` directly, even after the user has cancelled.
    pub async fn send_now(&mut self, bytes: &[u8]) {
        let _ = tokio::time::timeout(Duration::from_secs(2), self.outgoing.send(bytes.to_vec())).await;
    }
}
