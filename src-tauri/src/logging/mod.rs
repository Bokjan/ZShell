//! Session logs: what a terminal shows (remote output, echoed input, our own messages; not
//! ZMODEM data), written to a file by the backend as it is shown. Plain text by default:
//! control sequences are dropped, and backspaces and carriage returns are applied to the
//! line, so a shell's line editing and progress bars come out as the screen showed them.
//!
//! Each log has a writer thread, given the output through a bounded queue: a slow log folder
//! (a network drive, a sleeping disk) never holds up the session. When the queue is full,
//! output is left out of the log, and a line in it says how much.
//!
//! New log files are readable by their owner only (on Unix), as the other files ZShell
//! writes are: they can hold what was typed and shown, passwords echoed by mistake
//! included, in a folder the user may share.
//!
//! Logs that ZShell writes are listed in `logs.json`, so cleaning up (by age, for a session,
//! or all) only ever deletes files it created, never others in the log folder, and finds a
//! session's logs after it was renamed.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use chrono::Local;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::local_name::{create_unique, local_file_name};
use crate::model::{LogFormat, LogSettings};
use crate::persist::{JsonFile, SetAside};

mod index;
mod text;
pub mod time_format;

use index::Active;
pub use index::{LogSummary, Logs};
use text::Line;
pub use time_format::{TimeFormat, TimePart};

/// What a session's log is named after.
#[derive(Clone, Debug, Default)]
pub struct LogInfo {
    pub session: String,
    pub host: String,
    pub user: String,
    /// The saved session, if any; its logs can be deleted together.
    pub profile: Option<String>,
}

/// How a new session's log starts, as the frontend asks.
#[derive(Clone, Debug, Default, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum LogOpen {
    /// As the session (or, for local terminals, the settings) says.
    #[default]
    Auto,
    /// Go on with this file: the tab reconnected while logging.
    Append { path: PathBuf },
    /// Not logged: stopped by hand in this tab.
    Off,
}

/// How long written output may wait in the buffer before it reaches the file: a log is
/// readable while it is written (`tail -f`), without a flush (a system call, and a disk
/// write) for every chunk of output.
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Chunks of output a log's writer thread may have waiting.
const QUEUE: usize = 256;

/// What a log's writer thread is given.
enum Chunk {
    Output(Vec<u8>),
    /// This many bytes of output were left out: the queue was full.
    Dropped(usize),
}

/// The way to a log's writer thread.
struct Queue {
    chunks: SyncSender<Chunk>,
    /// Bytes left out since the last chunk queued.
    dropped: usize,
    path: PathBuf,
    /// Why the writer stopped, if writing failed.
    failure: Arc<Mutex<Option<String>>>,
    /// Where `dropped` goes when the log stops, for the writer to note at the end.
    left_out: Arc<AtomicUsize>,
    /// The log starts each line with the time.
    stamps_lines: bool,
}

impl Drop for Queue {
    fn drop(&mut self) {
        // Before `chunks` is dropped, which ends the writer.
        self.left_out.store(self.dropped, Ordering::Release);
    }
}

/// A session's log, if one is being written; shared by everything that writes to the
/// terminal (see `SessionSink::write`).
pub struct LogSlot {
    pub info: LogInfo,
    queue: Mutex<Option<Queue>>,
}

impl LogSlot {
    pub fn new(info: LogInfo) -> Arc<Self> {
        Arc::new(Self { info, queue: Mutex::new(None) })
    }

    /// Logs what the terminal is about to show, without waiting: left out when the writer
    /// is that far behind. A writer that failed (the disk is full, or gone) stops the log;
    /// the error is returned once, for the session to report.
    pub fn write(&self, bytes: &[u8]) -> Option<Error> {
        let mut slot = self.queue.lock().unwrap();
        let queue = slot.as_mut()?;
        let send = |chunk| match queue.chunks.try_send(chunk) {
            Ok(()) => Some(true),
            Err(TrySendError::Full(_)) => Some(false),
            Err(TrySendError::Disconnected(_)) => None,
        };
        // What was left out is noted first; once it is, what is left out next is counted
        // anew.
        let mut sent = Some(true);
        if queue.dropped > 0 {
            sent = send(Chunk::Dropped(queue.dropped));
            if sent == Some(true) {
                queue.dropped = 0;
            }
        }
        if sent == Some(true) {
            sent = send(Chunk::Output(bytes.to_vec()));
        }
        match sent {
            Some(true) => {}
            Some(false) => queue.dropped += bytes.len(),
            None => {
                let queue = slot.take()?;
                let error = Error::new("log.writeFailed").param("path", queue.path.display());
                return Some(match queue.failure.lock().unwrap().take() {
                    Some(failure) => error.detail(failure),
                    None => error,
                });
            }
        }
        None
    }

    /// Whether the log starts each line with the time, which our own messages then leave
    /// out (see `SessionSink::notice`).
    pub fn stamps_lines(&self) -> bool {
        self.queue.lock().unwrap().as_ref().is_some_and(|queue| queue.stamps_lines)
    }
}

struct LogWriter {
    file: BufWriter<File>,
    /// Plain text conversion; `None` for raw logs.
    text: Option<(vte::Parser, Line)>,
    /// When the file was last flushed, and whether something was written since.
    flushed: Instant,
    dirty: bool,
    /// Keeps the file out of cleanups while it is written.
    _active: Active,
    /// Where `run` leaves why writing failed.
    failure: Arc<Mutex<Option<String>>>,
    /// Output left out after the last chunk, once the log has stopped (see `Queue`).
    left_out: Arc<AtomicUsize>,
}

impl LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match &mut self.text {
            Some((parser, line)) => {
                parser.advance(line, bytes);
                self.file.write_all(&line.out)?;
                line.out.clear();
            }
            None => self.file.write_all(bytes)?,
        }
        self.dirty = true;
        match self.flushed.elapsed() >= FLUSH_INTERVAL {
            true => self.flush(),
            // What stays in the buffer is flushed when no more comes (see `LogWriter::run`).
            false => Ok(()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushed = Instant::now();
        self.dirty = false;
        self.file.flush()
    }

    /// A line of our own (a header, a reconnection), outside the terminal's output.
    fn note(&mut self, text: &str) -> std::io::Result<()> {
        if let Some((_, line)) = &mut self.text {
            line.finish();
            self.file.write_all(&line.out)?;
            line.out.clear();
        }
        writeln!(self.file, "{text}")?;
        self.flush()
    }
}

impl LogWriter {
    /// Writes what the session gives until it stops the log (or a write fails), flushing
    /// once output pauses for [`FLUSH_INTERVAL`]. With nothing to flush it sleeps until there
    /// is output, rather than waking every interval for the life of the session.
    fn run(mut self, chunks: std::sync::mpsc::Receiver<Chunk>) {
        loop {
            let next = match self.dirty {
                true => chunks.recv_timeout(FLUSH_INTERVAL),
                false => chunks.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            let written = match next {
                Ok(Chunk::Output(bytes)) => self.write(&bytes),
                Ok(Chunk::Dropped(bytes)) => self.note(&t!("log.dropped", bytes = bytes)),
                Err(RecvTimeoutError::Timeout) if self.dirty => self.flush(),
                Err(RecvTimeoutError::Timeout) => Ok(()),
                Err(RecvTimeoutError::Disconnected) => {
                    // Nothing is told of a failure now: the session may be gone, or stopped
                    // logging.
                    if let Err(e) = self.close() {
                        eprintln!("session log {}: {e}", self._active.path.display());
                    }
                    return;
                }
            };
            if let Err(e) = written {
                *self.failure.lock().unwrap() = Some(e.to_string());
                return;
            }
        }
    }

    /// Ends the log: notes output left out at the end, and writes out the unfinished line.
    fn close(&mut self) -> std::io::Result<()> {
        match self.left_out.load(Ordering::Acquire) {
            0 => {
                if let Some((_, line)) = &mut self.text {
                    line.finish();
                    self.file.write_all(&line.out)?;
                    line.out.clear();
                }
                self.flush()
            }
            bytes => self.note(&t!("log.dropped", bytes = bytes)),
        }
    }
}

/// For a writer that never ran, or stopped on a failure; `close` leaves nothing to do.
impl Drop for LogWriter {
    fn drop(&mut self) {
        if let Some((_, line)) = &mut self.text {
            line.finish();
            let _ = self.file.write_all(&line.out);
        }
        let _ = self.file.flush();
    }
}

#[cfg(test)]
mod tests;
