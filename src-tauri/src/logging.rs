//! Session logs: what a terminal shows (remote output, echoed input, our own messages; not
//! ZMODEM data), written to a file by the backend as it is shown. Plain text by default:
//! control sequences are dropped, and backspaces and carriage returns are applied to the
//! line, so a shell's line editing and progress bars come out as the screen showed them.
//!
//! Logs that ZShell writes are listed in `logs.json`, so cleaning up (by age, for a session,
//! or all) only ever deletes files it created, never others in the log folder, and finds a
//! session's logs after it was renamed.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime};

use chrono::Local;
use unicode_width::UnicodeWidthChar;
use serde::{Deserialize, Serialize};

use crate::config::{load_json, write_json_atomic, SetAside};
use crate::error::{Error, Result};
use crate::local_name::create_unique_file;
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
#[derive(Clone, Debug, Default, Deserialize)]
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

type SharedWriter = Arc<Mutex<Option<LogWriter>>>;

/// A session's log, if one is being written; shared by everything that writes to the
/// terminal (see `SessionSink::write`).
pub struct LogSlot {
    pub info: LogInfo,
    writer: SharedWriter,
}

impl LogSlot {
    pub fn new(info: LogInfo) -> Arc<Self> {
        Arc::new(Self { info, writer: SharedWriter::default() })
    }

    /// Logs what the terminal is about to show. A failing write (the disk is full, or gone)
    /// stops the log rather than retrying on every chunk.
    pub fn write(&self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        if let Some(log) = writer.as_mut() {
            if log.write(bytes).is_err() {
                *writer = None;
            }
        }
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
            // What stays in the buffer is flushed by `Logs::flush_idle` if no more comes.
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

/// Removes its file from the active set when the log closes.
struct Active {
    path: PathBuf,
    set: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.path);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSummary {
    pub count: usize,
    pub bytes: u64,
}

/// The logs ZShell has written, and the ones being written now.
pub struct Logs {
    index_path: PathBuf,
    default_dir: PathBuf,
    index: Mutex<Vec<Entry>>,
    active: Arc<Mutex<HashSet<PathBuf>>>,
    /// The writers of the sessions that have had a log, for [`Logs::flush_idle`].
    writers: Mutex<Vec<Weak<Mutex<Option<LogWriter>>>>>,
}

impl Logs {
    pub fn load(index_path: PathBuf, default_dir: PathBuf, set_aside: &SetAside) -> Self {
        let index = load_json(&index_path, set_aside);
        Self { index_path, default_dir, index: Mutex::new(index), active: Arc::default(), writers: Mutex::default() }
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
            .and_then(|_| create_unique_file(&path))
            .map_err(|e| Error::new("log.createFailed").param("path", path.display()).detail(e))?;
        let info = &slot.info;
        let address = if info.user.is_empty() { info.host.clone() } else { format!("{}@{}", info.user, info.host) };
        let header = t!("log.started", session = info.session, address = address, time = now());
        self.install(slot, path.clone(), file, settings, &header)?;
        self.remember(Entry { path: path.clone(), profile: info.profile.clone() });
        Ok(path)
    }

    /// Goes on with `path` after a reconnection, marking where the new connection starts.
    ///
    /// The path comes from the frontend, which was given it when the log started; one that
    /// isn't a log ZShell wrote (or that was deleted since) starts a new log rather than
    /// writing to whatever file it names.
    fn append(&self, slot: &LogSlot, path: PathBuf, settings: &LogSettings) -> Result<PathBuf> {
        if !self.index.lock().unwrap().iter().any(|entry| entry.path == path) || !path.is_file() {
            return self.start(slot, settings);
        }
        let file = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| Error::new("log.createFailed").param("path", path.display()).detail(e))?;
        self.install(slot, path.clone(), file, settings, &t!("log.resumed", time = now()))?;
        Ok(path)
    }

    fn install(&self, slot: &LogSlot, path: PathBuf, file: File, settings: &LogSettings, note: &str) -> Result<()> {
        self.active.lock().unwrap().insert(path.clone());
        let active = Active { path: path.clone(), set: self.active.clone() };
        let text = (settings.format == LogFormat::Text)
            .then(|| (vte::Parser::new(), Line { timestamps: settings.timestamps, ..Line::default() }));
        let mut writer = LogWriter { file: BufWriter::new(file), text, flushed: Instant::now(), dirty: false, _active: active };
        writer.note(note).map_err(|e| Error::new("log.createFailed").param("path", path.display()).detail(e))?;
        *slot.writer.lock().unwrap() = Some(writer);
        let mut writers = self.writers.lock().unwrap();
        if !writers.iter().any(|w| w.as_ptr() == Arc::as_ptr(&slot.writer)) {
            writers.push(Arc::downgrade(&slot.writer));
        }
        Ok(())
    }

    /// Flushes what logs were given since their last flush, if it was [`FLUSH_INTERVAL`]
    /// ago: the output stopped before the next flush was due. Called every so often.
    pub fn flush_idle(&self) {
        self.writers.lock().unwrap().retain(|writer| {
            let Some(writer) = writer.upgrade() else { return false };
            let mut writer = writer.lock().unwrap();
            if let Some(log) = writer.as_mut() {
                if log.dirty && log.flushed.elapsed() >= FLUSH_INTERVAL && log.flush().is_err() {
                    *writer = None;
                }
            }
            true
        });
    }

    pub fn stop(&self, slot: &LogSlot) {
        slot.writer.lock().unwrap().take();
    }

    fn remember(&self, entry: Entry) {
        let mut index = self.index.lock().unwrap();
        index.push(entry);
        let _ = write_json_atomic(&self.index_path, &*index);
    }

    /// The logs that still exist, forgetting the others (deleted by hand).
    fn existing(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        let mut index = self.index.lock().unwrap();
        let before = index.len();
        index.retain(|entry| entry.path.is_file());
        if index.len() != before {
            let _ = write_json_atomic(&self.index_path, &*index);
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
        let active = self.active.lock().unwrap().clone();
        let mut index = self.existing();
        let mut deleted = 0;
        index.retain(|entry| {
            if !matches(entry) || active.contains(&entry.path) || std::fs::remove_file(&entry.path).is_err() {
                return true;
            }
            deleted += 1;
            false
        });
        let _ = write_json_atomic(&self.index_path, &*index);
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

fn modified(path: &Path) -> Option<SystemTime> {
    path.metadata().and_then(|m| m.modified()).ok()
}

fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The template with the session's details and the time filled in, as a file name: path
/// separators and characters Windows doesn't allow become `_`.
fn file_name(template: &str, info: &LogInfo) -> String {
    let time = Local::now();
    let name = template
        .replace("{session}", &info.session)
        .replace("{host}", &info.host)
        .replace("{user}", &info.user)
        .replace("{date}", &time.format("%Y-%m-%d").to_string())
        .replace("{time}", &time.format("%H-%M-%S").to_string());
    let name: String =
        name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let name = name.trim().trim_matches('.');
    if name.is_empty() {
        "session.log".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(chunks: &[&[u8]]) -> String {
        let mut parser = vte::Parser::new();
        let mut line = Line::default();
        for chunk in chunks {
            parser.advance(&mut line, chunk);
        }
        line.finish();
        String::from_utf8(line.out).unwrap()
    }

    #[test]
    fn plain_text_follows_the_cursor_within_a_line() {
        // Colors, a corrected typo, a progress bar, CRLF line ends, a sequence split
        // across chunks.
        let log = text(&[
            b"\x1b[32muser@host\x1b[0m:~$ lss\x08 \x08 -l\r\n",
            b"10%\r50%\r100%\r\n",
            b"\x1b[1",
            b";31mred\x1b[0m done\r\n",
            // zsh redrawing its command line: back to the start, over the prompt, rewrite.
            b"% echo loggedx\x08\x1b[K\r\x1b[2Cecho logged\r\n",
            "中文 ok\r\x1b[5C!\r\n".as_bytes(),
            b"$ ",
        ]);
        assert_eq!(log, "user@host:~$ ls -l\n100%\nred done\n% echo logged\n中文 !k\n$\n");
    }

    #[test]
    fn long_lines_and_far_cursor_moves_stay_bounded() {
        let long = "a".repeat(3 * MAX_COLUMNS + 10);
        assert_eq!(text(&[long.as_bytes(), b"\r\nnext\r\n"]), format!("{long}\nnext\n"));

        let mut parser = vte::Parser::new();
        let mut line = Line::default();
        for _ in 0..1000 {
            parser.advance(&mut line, b"\x1b[65535C\x1b[65535@x\x1b[65535X\t");
        }
        assert!(line.cells.len() <= MAX_COLUMNS && line.cursor <= MAX_COLUMNS);
        parser.advance(&mut line, b"e");
        parser.advance(&mut line, "\u{301}".repeat(10_000).as_bytes());
        assert!(line.cells.iter().all(|cell| cell.len() <= MAX_CELL_LEN));
        assert!(line.out.len() < 100 * MAX_COLUMNS, "{}", line.out.len());
    }

    #[test]
    fn file_names_are_safe() {
        let info = LogInfo { session: "web/01: prod".into(), host: "10.0.0.1".into(), user: "root".into(), profile: None };
        let name = file_name("{session}_{user}@{host}.log", &info);
        assert_eq!(name, "web_01_ prod_root@10.0.0.1.log");
        assert_eq!(file_name("..", &info), "session.log");
    }

    #[test]
    fn output_reaches_the_file_within_the_flush_interval() {
        let dir = std::env::temp_dir().join(format!("zshell-logs-flush-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
        let settings = LogSettings { format: LogFormat::Raw, ..LogSettings::default() };
        let slot = LogSlot::new(LogInfo::default());
        let path = logs.start(&slot, &settings).unwrap();
        let header = std::fs::read(&path).unwrap();

        // Not flushed on every chunk, nor before the interval has passed.
        slot.write(b"one ");
        slot.write(b"two");
        logs.flush_idle();
        assert_eq!(std::fs::read(&path).unwrap(), header);
        std::thread::sleep(FLUSH_INTERVAL);
        logs.flush_idle();
        assert_eq!(std::fs::read(&path).unwrap(), [header.as_slice(), b"one two"].concat());

        // A slot that has gone is forgotten.
        drop(slot);
        logs.flush_idle();
        assert!(logs.writers.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_appends_and_deletes_only_its_own_logs() {
        let dir = std::env::temp_dir().join(format!("zshell-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let logs = Logs::load(dir.join("logs.json"), dir.join("Logs"), &SetAside::default());
        let settings = LogSettings::default();
        let info = LogInfo { session: "web".into(), host: "h".into(), user: "u".into(), profile: Some("p1".into()) };

        let slot = LogSlot::new(info.clone());
        let path = logs.start(&slot, &settings).unwrap();
        slot.write(b"\x1b[1mhello\x1b[0m\r\n");
        logs.stop(&slot);
        // A reconnection goes on with the same file.
        let slot = LogSlot::new(info);
        logs.open(&slot, LogOpen::Append { path: path.clone() }, false, &settings).unwrap().unwrap();
        slot.write(b"again\r\n");

        // Not deleted while written; another file in the folder is never touched.
        std::fs::write(dir.join("Logs").join("mine.txt"), "keep").unwrap();
        assert_eq!(logs.delete_for("p1"), 0);
        drop(slot);
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!((lines[1], lines[3]), ("hello", "again"));

        // Going on with a file that isn't one of the logs starts a new log instead.
        let other = LogSlot::new(LogInfo { profile: None, ..LogInfo::default() });
        let mine = dir.join("Logs").join("mine.txt");
        let started = logs.open(&other, LogOpen::Append { path: mine.clone() }, false, &settings).unwrap().unwrap();
        assert_ne!(started, mine);
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "keep");
        drop(other);
        std::fs::remove_file(started).unwrap();
        assert_eq!(logs.count("p1"), 1);
        assert_eq!(logs.delete_for("p1"), 1);
        assert!(!path.exists() && dir.join("Logs").join("mine.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
