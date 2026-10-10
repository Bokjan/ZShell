//! The app's JSON files (sessions, settings, quick commands, the log index): read at startup
//! without failing on a damaged or unreadable file, and written so that a crash or a full disk
//! never leaves one half written. See "读不出的文件" in `docs/ARCHITECTURE.md`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use ts_rs::TS;

use crate::error::{Error, Result};

/// A store's JSON file. One that couldn't be read at startup is never written over.
pub struct JsonFile {
    path: PathBuf,
    found: Found,
}

/// What a store's file was like at startup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    /// Read, or not there yet.
    Read,
    /// Not readable as the store's data, and moved aside: the store starts a new file.
    SetAside,
    /// Not readable, or not movable: left as it is, and not written while the app runs.
    LeftAlone,
}

/// Reading a file again after an error, for one that something else (an antivirus, a sync
/// client) has open for a moment.
const READ_ATTEMPTS: u32 = 3;
const READ_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

impl JsonFile {
    /// Reads the file at `path`, or gives the default if it doesn't exist yet.
    ///
    /// A file that can be read but not as `T` (cut short by a crash, edited by hand, or
    /// written by a newer version) is moved aside to `<name>.bad-<time>` and the store starts
    /// from the default: otherwise the app would fail to start, or the next save would replace
    /// the file. One that can't be read at all, or moved, is left where it is and never written
    /// (a restart reads it again). `set_aside` collects these files for the frontend to report.
    pub fn load<T: serde::de::DeserializeOwned + Default>(path: PathBuf, set_aside: &SetAside) -> (Self, T) {
        let mut attempt = 1;
        let bytes = loop {
            match fs::read(&path) {
                Ok(bytes) => break bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self { path, found: Found::Read }, T::default()),
                Err(_) if attempt < READ_ATTEMPTS => {
                    attempt += 1;
                    std::thread::sleep(READ_RETRY_DELAY);
                }
                Err(e) => {
                    set_aside.add(&path, &e.into(), false);
                    return (Self { path, found: Found::LeftAlone }, T::default());
                }
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(value) => (Self { path, found: Found::Read }, value),
            Err(e) => {
                let found = if set_aside.add(&path, &e.into(), true) { Found::SetAside } else { Found::LeftAlone };
                (Self { path, found }, T::default())
            }
        }
    }

    pub fn write(&self, value: &impl Serialize) -> Result<()> {
        self.stage(value)?.commit()
    }

    /// Writes `value` if it isn't `current`, what the file holds.
    pub fn write_changed<T: Serialize>(&self, current: &T, value: &T) -> Result<()> {
        if serde_json::to_value(current).ok() == serde_json::to_value(value).ok() {
            return Ok(());
        }
        self.write(value)
    }

    /// Writes `value` next to the file, to be put in place by [`Staged::commit`].
    pub fn stage(&self, value: &impl Serialize) -> Result<Staged> {
        if self.found == Found::LeftAlone {
            let name = self.path.file_name().unwrap_or_default().to_string_lossy();
            return Err(Error::new("config.unreadable").param("name", name));
        }
        stage_json(&self.path, value).map_err(|e| write_failed(&self.path, e))
    }
}

/// Files that could not be read at startup (see [`JsonFile::load`]).
#[derive(Default)]
pub struct SetAside(Mutex<Vec<SetAsideFile>>);

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetAsideFile {
    pub path: PathBuf,
    /// Where the file was moved; `None` if it was left as it is (see [`Found::LeftAlone`]).
    pub moved_to: Option<PathBuf>,
    pub error: String,
}

impl SetAside {
    /// Records a file that couldn't be read, moving it aside if `move_it`; returns whether
    /// it was moved.
    fn add(&self, path: &Path, error: &anyhow::Error, move_it: bool) -> bool {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let moved = path.with_file_name(format!("{name}.bad-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        let moved_to = move_it.then(|| fs::rename(path, &moved).ok().map(|()| moved)).flatten();
        let was_moved = moved_to.is_some();
        self.0.lock().unwrap().push(SetAsideFile { path: path.to_owned(), moved_to, error: format!("{error:#}") });
        was_moved
    }

    /// The files set aside, once: the frontend reports them when it starts.
    pub fn take(&self) -> Vec<SetAsideFile> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Writes `value` as pretty JSON, via a temporary file and a rename so that a crash never
/// leaves a truncated file behind. The data is flushed to disk before the rename: otherwise
/// a power loss can leave the renamed file empty (seen on NTFS).
pub fn write_json_atomic(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    stage_json(path, value)?.put_in_place()
}

fn write_failed(path: &Path, error: std::io::Error) -> Error {
    Error::new("config.writeFailed").param("path", path.display()).detail(error)
}

/// A file written next to its place under a temporary name and flushed to disk (see
/// [`write_json_atomic`]), put in place by [`Staged::commit`]. Dropped without that, the
/// temporary file is removed.
pub struct Staged {
    tmp: PathBuf,
    path: PathBuf,
    committed: bool,
}

fn stage_json(path: &Path, value: &impl Serialize) -> std::io::Result<Staged> {
    use std::io::Write;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let staged = Staged { tmp: path.with_extension("json.tmp"), path: path.to_owned(), committed: false };
    let mut file = fs::File::create(&staged.tmp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(staged)
}

impl Staged {
    /// Puts the file in place; `config.writeFailed` if it can't be.
    pub fn commit(self) -> Result<()> {
        let path = self.path.clone();
        self.put_in_place().map_err(|e| write_failed(&path, e))
    }

    fn put_in_place(mut self) -> std::io::Result<()> {
        fs::rename(&self.tmp, &self.path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.tmp);
        }
    }
}

/// Gives an id of its own to each empty one and each one used before (a hand-edited or
/// someone else's file), so that references go to the first. Made from the position, so that
/// reading the file again gives the same ids.
pub fn unique_ids<'a>(ids: impl Iterator<Item = &'a mut String>) {
    let ids: Vec<&mut String> = ids.collect();
    let mut seen: HashSet<String> = HashSet::new();
    for (index, id) in ids.into_iter().enumerate() {
        if id.is_empty() || seen.contains(id.as_str()) {
            *id = (0..).map(|n| format!("#{index}.{n}")).find(|candidate| !seen.contains(candidate)).unwrap();
        }
        seen.insert(id.clone());
    }
}
