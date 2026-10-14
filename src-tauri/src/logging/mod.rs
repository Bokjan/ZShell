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
use unicode_width::UnicodeWidthChar;

use crate::error::{Error, Result};
use crate::local_name::{create_unique, local_file_name};
use crate::persist::{JsonFile, SetAside};
use crate::settings::{LogFormat, LogSettings};

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
    /// once output pauses for [`FLUSH_INTERVAL`].
    fn run(mut self, chunks: std::sync::mpsc::Receiver<Chunk>) {
        loop {
            let written = match chunks.recv_timeout(FLUSH_INTERVAL) {
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

/// Columns kept for the line being edited. Output is not to be trusted with memory (a long
/// minified file, or cursor movements of up to 65535 columns each): text going past this
/// is passed on (see [`Line::spill`]) and the cursor stays within it.
const MAX_COLUMNS: usize = 4096;
/// Bytes kept on one column, for characters of no width (combining marks).
const MAX_CELL_LEN: usize = 32;

/// The line being shown, as plain text, with the cursor's column: carriage returns,
/// backspaces and the cursor movements and erasures a shell uses to edit its command line
/// are applied, so a line comes out as the screen showed it. Complete lines go to `out`.
#[derive(Default)]
struct Line {
    /// One per column; a wide character's second column is empty, and characters of no
    /// width (combining marks) join the one before.
    cells: Vec<String>,
    cursor: usize,
    /// Part of the line has already gone to `out`.
    spilled: bool,
    timestamps: bool,
    out: Vec<u8>,
}

impl Line {
    fn end(&mut self) {
        self.start_out();
        self.out.extend(self.cells.concat().trim_end().as_bytes());
        self.out.push(b'\n');
        self.cells.clear();
        self.cursor = 0;
        self.spilled = false;
    }

    /// Passes on what the line has so far, without ending it, to go on at column 0: for a
    /// line longer than `MAX_COLUMNS`, whose start is past editing. Trailing blanks are
    /// dropped, as at the end of a line: cursor movements alone (a few bytes each) would
    /// otherwise turn into whole lines of spaces in the file.
    fn spill(&mut self) {
        self.start_out();
        self.out.extend(self.cells.concat().trim_end().as_bytes());
        self.cells.clear();
        self.cursor = 0;
        self.spilled = true;
    }

    fn start_out(&mut self) {
        if self.timestamps && !self.spilled {
            self.out.extend(Local::now().format("[%Y-%m-%d %H:%M:%S] ").to_string().as_bytes());
        }
    }

    /// Ends a line left unfinished (at a note, or when the log closes).
    fn finish(&mut self) {
        if self.spilled || !self.cells.concat().trim().is_empty() {
            self.end();
        }
        self.cells.clear();
        self.cursor = 0;
    }

    /// Moves the cursor to column `to`, within `MAX_COLUMNS`, padding the line to reach it.
    fn move_to(&mut self, to: usize) {
        self.cursor = to.min(MAX_COLUMNS);
        self.pad(self.cursor);
    }

    /// Makes the line reach column `to`, with spaces.
    fn pad(&mut self, to: usize) {
        if self.cells.len() < to {
            self.cells.resize(to, " ".to_owned());
        }
    }
}

impl vte::Perform for Line {
    fn print(&mut self, c: char) {
        match c.width().unwrap_or(0) {
            0 => {
                if let Some(cell) = self.cursor.checked_sub(1).and_then(|i| self.cells.get_mut(i)) {
                    if cell.len() + c.len_utf8() <= MAX_CELL_LEN {
                        cell.push(c);
                    }
                }
            }
            width => {
                if self.cursor + width > MAX_COLUMNS {
                    self.spill();
                }
                self.pad(self.cursor + width);
                self.cells[self.cursor] = c.to_string();
                if width == 2 {
                    self.cells[self.cursor + 1].clear();
                }
                self.cursor += width;
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.end(),
            b'\r' => self.cursor = 0,
            0x08 => self.cursor = self.cursor.saturating_sub(1),
            b'\t' => self.move_to((self.cursor / 8 + 1) * 8),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let first = params.iter().next().map_or(0, |p| p[0] as usize);
        let count = first.max(1);
        match action {
            // Cursor forward, back, to a column.
            'C' => self.move_to(self.cursor + count),
            'D' => self.cursor = self.cursor.saturating_sub(count),
            'G' => self.move_to(count - 1),
            // Erase to the end of the line, from its start, or all of it.
            'K' => match first {
                0 => self.cells.truncate(self.cursor),
                1 => self.cells.iter_mut().take(self.cursor + 1).for_each(|cell| *cell = " ".to_owned()),
                _ => self.cells.clear(),
            },
            // Delete, insert or blank characters at the cursor.
            'P' => {
                let end = (self.cursor + count).min(self.cells.len());
                if self.cursor < end {
                    self.cells.drain(self.cursor..end);
                }
            }
            // What is pushed past the last column is lost, as on a screen.
            '@' => {
                self.pad(self.cursor);
                let count = count.min(MAX_COLUMNS - self.cursor);
                self.cells.splice(self.cursor..self.cursor, std::iter::repeat_n(" ".to_owned(), count));
                self.cells.truncate(MAX_COLUMNS);
            }
            'X' => {
                let end = (self.cursor + count).min(MAX_COLUMNS);
                self.pad(end);
                self.cells[self.cursor..end].iter_mut().for_each(|cell| *cell = " ".to_owned());
            }
            _ => {}
        }
    }
}

/// The logs being written, by path, with how many writers each has: a reconnection's writer
/// may start before the previous connection's has finished.
#[derive(Default)]
struct ActiveLogs {
    paths: Mutex<HashMap<PathBuf, usize>>,
    closed: Condvar,
}

impl ActiveLogs {
    /// Waits until nothing writes `path` any more, for at most `timeout`.
    #[cfg(test)]
    fn wait_closed(&self, path: &Path, timeout: Duration) {
        let paths = self.paths.lock().unwrap();
        let _paths = self.closed.wait_timeout_while(paths, timeout, |paths| paths.contains_key(path)).unwrap();
    }

    /// Waits until one writer of `path` is left: the caller's.
    fn wait_alone(&self, path: &Path) {
        let paths = self.paths.lock().unwrap();
        let _paths = self.closed.wait_while(paths, |paths| paths.get(path).is_some_and(|&count| count > 1)).unwrap();
    }

    /// Counts a writer of `path` until the returned value is dropped.
    fn add(self: &Arc<Self>, path: PathBuf) -> Active {
        *self.paths.lock().unwrap().entry(path.clone()).or_default() += 1;
        Active { path, set: self.clone() }
    }

    fn contains(&self, path: &Path) -> bool {
        self.paths.lock().unwrap().contains_key(path)
    }
}

/// A writer of a log, counted in the active set while it exists.
struct Active {
    path: PathBuf,
    set: Arc<ActiveLogs>,
}

impl Drop for Active {
    fn drop(&mut self) {
        let mut paths = self.set.paths.lock().unwrap();
        if let Some(count) = paths.get_mut(&self.path) {
            *count -= 1;
            if *count == 0 {
                paths.remove(&self.path);
            }
        }
        drop(paths);
        self.set.closed.notify_all();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogSummary {
    pub count: usize,
    pub bytes: u64,
}

/// The logs ZShell has written, and the ones being written now.
pub struct Logs {
    index_file: JsonFile,
    default_dir: PathBuf,
    index: Mutex<Vec<Entry>>,
    active: Arc<ActiveLogs>,
}

impl Logs {
    pub fn load(index_path: PathBuf, default_dir: PathBuf, set_aside: &SetAside) -> Self {
        let (index_file, index) = JsonFile::load(index_path, set_aside);
        Self { index_file, default_dir, index: Mutex::new(index), active: Arc::default() }
    }

    pub fn directory(&self, settings: &LogSettings) -> PathBuf {
        if settings.directory.is_empty() {
            self.default_dir.clone()
        } else {
            PathBuf::from(&settings.directory)
        }
    }

    /// Starts a log for a new session as `how` says; `auto` is whether the session (or the
    /// settings) record it automatically. `None` when it isn't logged.
    pub fn open(&self, slot: &LogSlot, how: LogOpen, auto: bool, settings: &LogSettings) -> Option<Result<PathBuf>> {
        match how {
            LogOpen::Auto if auto => Some(self.start(slot, settings)),
            LogOpen::Append { path } => Some(self.append(slot, path, settings)),
            _ => None,
        }
    }

    /// Starts a new log file, named after the session, the template and the time.
    pub fn start(&self, slot: &LogSlot, settings: &LogSettings) -> Result<PathBuf> {
        let dir = self.directory(settings);
        let path = dir.join(file_name(&settings.file_name, &slot.info));
        let (path, file) = std::fs::create_dir_all(&dir)
            .and_then(|_| create_unique(&path, create_private))
            .map_err(|e| Error::new("log.createFailed").param("path", path.display()).detail(e))?;
        let info = &slot.info;
        let address = if info.user.is_empty() { info.host.clone() } else { format!("{}@{}", info.user, info.host) };
        let header = t!("log.started", session = info.session, address = address, time = now());
        // Counted as written before it is in the index, so no cleanup or deletion can see it.
        let active = self.active.add(path.clone());
        // A log missing from the index would never be cleaned up or deleted with the others:
        // none is written then.
        let started = self
            .remember(Entry { path: path.clone(), profile: info.profile.clone() })
            .and_then(|()| self.install(slot, path.clone(), file, active, settings, &header, false));
        if started.is_err() {
            self.forget(&path);
            let _ = std::fs::remove_file(&path);
        }
        started.map(|()| path)
    }

    /// Goes on with `path` after a reconnection, marking where the new connection starts.
    ///
    /// The path comes from the frontend, which was given it when the log started; one that
    /// isn't a log ZShell wrote (or that was deleted since) starts a new log rather than
    /// writing to whatever file it names.
    fn append(&self, slot: &LogSlot, path: PathBuf, settings: &LogSettings) -> Result<PathBuf> {
        let is_log = |index: &[Entry]| index.iter().any(|entry| entry.path == path) && path.is_file();
        if !is_log(&self.index.lock().unwrap()) {
            return self.start(slot, settings);
        }
        // Checked again, opened and counted as written under the index's lock, which
        // deleting holds: otherwise a log deleted in between would be written again, or one
        // about to be written deleted.
        let index = self.index.lock().unwrap();
        if !is_log(&index) {
            drop(index);
            return self.start(slot, settings);
        }
        let file = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| Error::new("log.createFailed").param("path", path.display()).detail(e))?;
        let active = self.active.add(path.clone());
        drop(index);
        // The previous connection's writer may still be writing what it was given (to a slow
        // network drive, say): this one waits for it, while output queues up meanwhile.
        self.install(slot, path.clone(), file, active, settings, &t!("log.resumed", time = now()), true)?;
        Ok(path)
    }

    /// Starts the writer thread with `note`, after the log's other writers have finished if
    /// `after_others` (otherwise a failure to write the note is returned).
    #[allow(clippy::too_many_arguments)]
    fn install(
        &self,
        slot: &LogSlot,
        path: PathBuf,
        file: File,
        active: Active,
        settings: &LogSettings,
        note: &str,
        after_others: bool,
    ) -> Result<()> {
        let text =
            (settings.format == LogFormat::Text).then(|| (vte::Parser::new(), Line { timestamps: settings.timestamps, ..Line::default() }));
        let (failure, left_out) = (Arc::new(Mutex::new(None)), Arc::new(AtomicUsize::new(0)));
        let mut writer = LogWriter {
            file: BufWriter::new(file),
            text,
            flushed: Instant::now(),
            dirty: false,
            _active: active,
            failure: failure.clone(),
            left_out: left_out.clone(),
        };
        let (chunks, queued) = sync_channel(QUEUE);
        if after_others {
            let (active, path, note) = (self.active.clone(), path.clone(), note.to_owned());
            thread::spawn(move || {
                active.wait_alone(&path);
                match writer.note(&note) {
                    Ok(()) => writer.run(queued),
                    Err(e) => *writer.failure.lock().unwrap() = Some(e.to_string()),
                }
            });
        } else {
            writer.note(note).map_err(|e| Error::new("log.createFailed").param("path", path.display()).detail(e))?;
            thread::spawn(move || writer.run(queued));
        }
        *slot.queue.lock().unwrap() = Some(Queue { chunks, dropped: 0, path, failure, left_out });
        Ok(())
    }

    /// Stops the session's log; its writer finishes what it was given.
    pub fn stop(&self, slot: &LogSlot) {
        slot.queue.lock().unwrap().take();
    }

    fn remember(&self, entry: Entry) -> Result<()> {
        let mut index = self.index.lock().unwrap();
        index.push(entry);
        let written = self.index_file.write(&*index);
        if written.is_err() {
            index.pop();
        }
        written
    }

    fn forget(&self, path: &Path) {
        let mut index = self.index.lock().unwrap();
        let before = index.len();
        index.retain(|entry| entry.path != path);
        if index.len() != before {
            let _ = self.index_file.write(&*index);
        }
    }

    /// The logs that still exist, forgetting the others (deleted by hand). Here and after
    /// deleting, the index is only written for what is gone: if that fails, it is forgotten
    /// again next time.
    fn existing(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        let mut index = self.index.lock().unwrap();
        let before = index.len();
        index.retain(|entry| entry.path.is_file());
        if index.len() != before {
            let _ = self.index_file.write(&*index);
        }
        index
    }

    pub fn summary(&self) -> LogSummary {
        let index = self.existing();
        let bytes = index.iter().filter_map(|entry| entry.path.metadata().ok()).map(|m| m.len()).sum();
        LogSummary { count: index.len(), bytes }
    }

    /// How many logs a saved session has.
    pub fn count(&self, profile: &str) -> usize {
        self.existing().iter().filter(|entry| entry.profile.as_deref() == Some(profile)).count()
    }

    /// Deletes the logs that `matches`, except those being written; returns how many.
    fn delete(&self, matches: impl Fn(&Entry) -> bool) -> usize {
        // Under the index's lock, which going on with a log takes to count it (see `append`).
        let mut index = self.existing();
        let mut deleted = 0;
        index.retain(|entry| {
            if !matches(entry) || self.active.contains(&entry.path) || std::fs::remove_file(&entry.path).is_err() {
                return true;
            }
            deleted += 1;
            false
        });
        let _ = self.index_file.write(&*index);
        deleted
    }

    pub fn delete_all(&self) -> usize {
        self.delete(|_| true)
    }

    pub fn delete_for(&self, profile: &str) -> usize {
        self.delete(|entry| entry.profile.as_deref() == Some(profile))
    }

    /// Deletes logs last written more than `keep_days` days ago; 0 keeps everything.
    pub fn clean_up(&self, keep_days: u32) -> usize {
        if keep_days == 0 {
            return 0;
        }
        let Some(cutoff) = SystemTime::now().checked_sub(Duration::from_secs(u64::from(keep_days) * 86_400)) else {
            return 0;
        };
        self.delete(|entry| modified(&entry.path).is_some_and(|time| time < cutoff))
    }
}

/// A new log file, readable by its owner only.
fn create_private(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn modified(path: &Path) -> Option<SystemTime> {
    path.metadata().and_then(|m| m.modified()).ok()
}

fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The template with the session's details and the time filled in, as a file name: path
/// separators and characters Windows doesn't allow become `_`, and a Windows device name
/// (`NUL`, `COM1.log`) gets a `_` in front.
fn file_name(template: &str, info: &LogInfo) -> String {
    let time = Local::now();
    let name = template
        .replace("{session}", &info.session)
        .replace("{host}", &info.host)
        .replace("{user}", &info.user)
        .replace("{date}", &time.format("%Y-%m-%d").to_string())
        .replace("{time}", &time.format("%H-%M-%S").to_string());
    let name: String = name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    local_file_name(name.trim().trim_matches('.')).unwrap_or_else(|| "session.log".to_owned())
}

#[cfg(test)]
mod tests;
