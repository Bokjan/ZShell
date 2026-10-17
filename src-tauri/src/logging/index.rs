//! The logs ZShell has written (`logs.json`), the ones being written now, and cleaning them
//! up (see the module docs of `logging`).

use super::*;

/// The logs being written, by path, with how many writers each has: a reconnection's writer
/// may start before the previous connection's has finished.
#[derive(Default)]
pub(super) struct ActiveLogs {
    pub(super) paths: Mutex<HashMap<PathBuf, usize>>,
    closed: Condvar,
}

impl ActiveLogs {
    /// Waits until nothing writes `path` any more, for at most `timeout`.
    #[cfg(test)]
    pub(super) fn wait_closed(&self, path: &Path, timeout: Duration) {
        let paths = self.paths.lock().unwrap();
        let _paths = self.closed.wait_timeout_while(paths, timeout, |paths| paths.contains_key(path)).unwrap();
    }

    /// Waits until nothing writes any log, for at most `timeout`.
    pub(super) fn wait_empty(&self, timeout: Duration) {
        let paths = self.paths.lock().unwrap();
        let _paths = self.closed.wait_timeout_while(paths, timeout, |paths| !paths.is_empty()).unwrap();
    }

    /// Waits until one writer of `path` is left: the caller's.
    pub(super) fn wait_alone(&self, path: &Path) {
        let paths = self.paths.lock().unwrap();
        let _paths = self.closed.wait_while(paths, |paths| paths.get(path).is_some_and(|&count| count > 1)).unwrap();
    }

    /// Counts a writer of `path` until the returned value is dropped.
    pub(super) fn add(self: &Arc<Self>, path: PathBuf) -> Active {
        *self.paths.lock().unwrap().entry(path.clone()).or_default() += 1;
        Active { path, set: self.clone() }
    }

    pub(super) fn contains(&self, path: &Path) -> bool {
        self.paths.lock().unwrap().contains_key(path)
    }
}

/// A writer of a log, counted in the active set while it exists.
pub(super) struct Active {
    pub(super) path: PathBuf,
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
    pub(super) active: Arc<ActiveLogs>,
}

impl Logs {
    /// Waits until every log being written has been closed and written to the end, for at
    /// most `timeout`: as the app quits, once its sessions have closed.
    pub fn wait_all_closed(&self, timeout: Duration) {
        self.active.wait_empty(timeout);
    }

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
pub(super) fn file_name(template: &str, info: &LogInfo) -> String {
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
