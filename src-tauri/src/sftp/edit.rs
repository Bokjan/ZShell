//! Editing remote files in a local editor: the file is downloaded into a temporary folder and
//! opened, then watched; each save is reported to the frontend, which uploads it (through the
//! tab's current connection, so editing survives reconnects).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use ts_rs::TS;

use super::file_name;
use super::replace::RemoteTarget;
use crate::error::{Error, Result};
use crate::local_name::local_file_name;

/// How long a local copy goes without changing before a change is taken as a finished save,
/// so that one still being written is not uploaded half-way.
const SETTLE: Duration = Duration::from_millis(300);
/// How often a local copy is checked where the system can't tell when it changes.
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Edit folders left by an earlier run are deleted once untouched for this long; not right
/// away, since another instance of the app may still be using its own.
const KEEP_OLD: Duration = Duration::from_secs(24 * 60 * 60);

/// What a remote file looked like, to notice when someone else changed it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Stamp {
    size: u64,
    modified: Option<u32>,
}

/// What the watcher sends the frontend.
#[derive(Clone, Serialize, TS)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum EditEvent {
    /// The local copy was saved (and has stayed the same for a moment).
    Changed,
}

struct Edit {
    remote: String,
    local: PathBuf,
    /// The remote file as last downloaded or uploaded.
    remote_stamp: Mutex<Option<Stamp>>,
    /// Held for a whole upload: two at once would each see the other's partial write as
    /// someone else's change, and both truncate the file.
    uploading: tokio::sync::Mutex<()>,
    stopped: AtomicBool,
    /// Woken by changes in the local copy's folder, and to stop.
    wake: Arc<Notify>,
}

impl Edit {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }
}

/// Tells edits when their folders change: one watcher for all of them, since it is a thread on
/// most systems (and Linux limits how many there are). It watches each edit's folder, not the
/// file: editors that save by writing a new file and renaming it over the old one would leave
/// a watch on the file following the old one.
#[derive(Default)]
struct Watcher {
    /// Made with the first edit; none until then, or while the system won't make one.
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// The folders watched, as the system names them (see `watch`), with the edits to wake.
    /// Locked only briefly: the watcher's thread locks it for each change, and stopping a
    /// watch waits for that thread on macOS.
    folders: Arc<Mutex<HashMap<PathBuf, Vec<Arc<Notify>>>>>,
}

impl Watcher {
    /// Wakes `wake` on each change in `folder`, unless the system can't watch it.
    fn add(&self, folder: &Path, wake: &Arc<Notify>) -> bool {
        let mut watcher = self.watcher.lock().unwrap();
        if watcher.is_none() {
            let folders = self.folders.clone();
            *watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let folders = folders.lock().unwrap();
                // Changes may have been missed when the system's queue overflowed.
                let everywhere = event.as_ref().map_or(true, |event| event.need_rescan());
                let paths = event.map(|event| event.paths).unwrap_or_default();
                for (folder, wakes) in folders.iter() {
                    if everywhere || paths.iter().any(|path| path.starts_with(folder)) {
                        wakes.iter().for_each(|wake| wake.notify_one());
                    }
                }
            })
            .ok();
        }
        let Some(watcher) = watcher.as_mut() else { return false };
        let watched = self.folders.lock().unwrap().contains_key(folder);
        if !watched && watcher.watch(folder, RecursiveMode::NonRecursive).is_err() {
            return false;
        }
        self.folders.lock().unwrap().entry(folder.to_owned()).or_default().push(wake.clone());
        true
    }

    fn remove(&self, folder: &Path, wake: &Arc<Notify>) {
        let mut watcher = self.watcher.lock().unwrap();
        let mut folders = self.folders.lock().unwrap();
        let Some(wakes) = folders.get_mut(folder) else { return };
        wakes.retain(|other| !Arc::ptr_eq(other, wake));
        if wakes.is_empty() {
            folders.remove(folder);
            drop(folders);
            if let Some(watcher) = watcher.as_mut() {
                let _ = watcher.unwatch(folder);
            }
        }
    }
}

/// Files being edited, by the webview that opened each and the id its page chose. An edit
/// belongs to its page, like a transfer: only that page uploads, reopens or stops it, and it
/// stops when the page is gone (its local copy stays, as after [`Edits::stop`]).
pub struct Edits {
    root: PathBuf,
    edits: Mutex<HashMap<(String, String), Arc<Edit>>>,
    watcher: Arc<Watcher>,
}

impl Edits {
    pub fn new(root: PathBuf) -> Self {
        // Not before the first window: a folder deleted meanwhile is one no edit uses now.
        let old = root.clone();
        std::thread::spawn(move || remove_old(&old));
        Self { root, edits: Mutex::new(HashMap::new()), watcher: Arc::default() }
    }

    /// Where `remote` is downloaded to for editing, in a folder of its own so the editor
    /// shows the real file name.
    pub fn local_path(&self, edit_id: &str, remote: &str) -> Result<PathBuf> {
        let Some(name) = local_file_name(file_name(remote)) else {
            return Err(Error::new("transfer.invalidName").param("name", remote));
        };
        if !super::is_folder_id(edit_id) {
            return Err(Error::new("transfer.invalidPath").param("path", remote));
        }
        Ok(self.root.join(edit_id).join(name))
    }

    /// Starts watching a file downloaded to `local`, reporting saves through `events`.
    /// `remote_stamp`: the remote file's, taken before downloading it (see `remote_stamp`).
    pub fn start(
        &self,
        webview: &str,
        edit_id: &str,
        remote: String,
        local: PathBuf,
        remote_stamp: Option<Stamp>,
        events: Channel<EditEvent>,
    ) {
        let edit = Arc::new(Edit {
            remote,
            local,
            remote_stamp: Mutex::new(remote_stamp),
            uploading: tokio::sync::Mutex::new(()),
            stopped: AtomicBool::new(false),
            wake: Arc::default(),
        });
        if let Some(old) = self.edits.lock().unwrap().insert((webview.to_owned(), edit_id.to_owned()), edit.clone()) {
            old.stop();
        }
        tauri::async_runtime::spawn(watch(edit, self.watcher.clone(), events));
    }

    fn get(&self, webview: &str, edit_id: &str) -> Result<Arc<Edit>> {
        let key = (webview.to_owned(), edit_id.to_owned());
        self.edits.lock().unwrap().get(&key).cloned().ok_or_else(|| Error::new("edit.notFound"))
    }

    pub fn local(&self, webview: &str, edit_id: &str) -> Result<PathBuf> {
        Ok(self.get(webview, edit_id)?.local.clone())
    }

    /// Uploads the local copy over the remote file. Unless `force`, fails with
    /// `edit.conflict` when the remote file changed since it was downloaded or last uploaded.
    pub async fn upload(&self, sftp: &SftpSession, webview: &str, edit_id: &str, force: bool) -> Result<()> {
        let edit = self.get(webview, edit_id)?;
        let _uploading = edit.uploading.lock().await;
        let remote = &edit.remote;
        let expected = *edit.remote_stamp.lock().unwrap();
        if !force && remote_stamp(sftp, remote).await != expected {
            return Err(Error::new("edit.conflict").param("name", file_name(remote)));
        }
        let read_failed = |e| Error::new("transfer.readFailed").param("path", edit.local.display()).detail(e);
        let mut local = tokio::fs::File::open(&edit.local).await.map_err(read_failed)?;
        let result: anyhow::Result<()> = async {
            // Replaced only once complete, keeping its owner and mode (see `replace`).
            let mut target = RemoteTarget::create(sftp, remote).await?;
            let written: anyhow::Result<()> = async {
                // Streamed: the file may be large.
                let mut buf = vec![0; 256 << 10];
                loop {
                    let n = local.read(&mut buf).await.map_err(read_failed)?;
                    if n == 0 {
                        break;
                    }
                    target.file.write_all(&buf[..n]).await?;
                }
                Ok(())
            }
            .await;
            match written {
                Ok(()) => target.finish(sftp).await,
                Err(e) => {
                    target.abandon(sftp).await;
                    Err(e)
                }
            }
        }
        .await;
        result.map_err(|e| Error::from(e.context(Error::new("transfer.uploadFailed").param("path", edit.local.display()))))?;
        *edit.remote_stamp.lock().unwrap() = remote_stamp(sftp, remote).await;
        Ok(())
    }

    /// Stops watching; the local copy stays until a later run cleans it up, in case the
    /// editor still has unsaved changes.
    pub fn stop(&self, webview: &str, edit_id: &str) {
        if let Some(edit) = self.edits.lock().unwrap().remove(&(webview.to_owned(), edit_id.to_owned())) {
            edit.stop();
        }
    }

    /// Stops the edits of `webview`, whose page is gone.
    pub fn stop_webview(&self, webview: &str) {
        self.edits.lock().unwrap().retain(|(owner, _), edit| {
            let theirs = owner == webview;
            if theirs {
                edit.stop();
            }
            !theirs
        });
    }
}

/// The remote file's size and modification time, which tell whether it changed since. Taken
/// before downloading it for editing: a change made during the download then shows as one.
pub async fn remote_stamp(sftp: &SftpSession, remote: &str) -> Option<Stamp> {
    let metadata = sftp.metadata(remote).await.ok()?;
    Some(Stamp { size: metadata.size.unwrap_or(0), modified: metadata.mtime })
}

/// Reports each change of the local copy once it has stopped changing: after [`SETTLE`]
/// without changes in its folder, or where the system can't watch the folder, once it is the
/// same in two checks in a row.
async fn watch(edit: Arc<Edit>, watcher: Arc<Watcher>, events: Channel<EditEvent>) {
    let stamp = |metadata: std::fs::Metadata| (metadata.len(), metadata.modified().ok());
    let mut reported = tokio::fs::metadata(&edit.local).await.ok().map(stamp);
    let mut previous = reported;
    // The real path, which is what the system reports changes in (macOS's temporary folder is
    // behind a symlink).
    let folder = edit.local.parent().and_then(|folder| std::fs::canonicalize(folder).ok());
    let watched = folder.filter(|folder| watcher.add(folder, &edit.wake));
    while !edit.stopped.load(Ordering::Relaxed) {
        if watched.is_some() {
            edit.wake.notified().await;
            while tokio::time::timeout(SETTLE, edit.wake.notified()).await.is_ok() {}
        } else {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        // Missing for a moment while editors that save by renaming replace it.
        let Ok(metadata) = tokio::fs::metadata(&edit.local).await else { continue };
        let current = Some(stamp(metadata));
        let settled = watched.is_some() || current == previous;
        if settled && current != reported && !edit.stopped.load(Ordering::Relaxed) {
            reported = current;
            if events.send(EditEvent::Changed).is_err() {
                break;
            }
        }
        previous = current;
    }
    if let Some(folder) = watched {
        watcher.remove(&folder, &edit.wake);
    }
}

/// Kinds of files that the system's default action runs, installs or follows instead of
/// opening them in an application (the server names the file and writes what is in it, and
/// nothing marks the file as downloaded for Gatekeeper or SmartScreen to warn about).
#[cfg(target_os = "macos")]
const RUN_WHEN_OPENED: &[&str] = &[
    "action",
    "app",
    "command",
    "configprofile",
    "fileloc",
    "inetloc",
    "jar",
    "mobileconfig",
    "mpkg",
    "pkg",
    "prefpane",
    "saver",
    "shortcut",
    "terminal",
    "tool",
    "webloc",
    "workflow",
];
/// Besides what `PATHEXT` lists.
#[cfg(windows)]
const RUN_WHEN_OPENED: &[&str] = &[
    "ade",
    "adp",
    "app",
    "application",
    "appref-ms",
    "appx",
    "appxbundle",
    "bas",
    "bat",
    "chm",
    "cmd",
    "com",
    "cpl",
    "diagcab",
    "exe",
    "gadget",
    "hlp",
    "hta",
    "inf",
    "ins",
    "isp",
    "jar",
    "js",
    "jse",
    "library-ms",
    "lnk",
    "mde",
    "msc",
    "msh",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "pif",
    "ps1",
    "ps1xml",
    "ps2",
    "psc1",
    "psd1",
    "psm1",
    "py",
    "pyw",
    "pyz",
    "reg",
    "scf",
    "scr",
    "sct",
    "search-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    "shb",
    "url",
    "vb",
    "vbe",
    "vbs",
    "website",
    "ws",
    "wsc",
    "wsf",
    "wsh",
    "xbap",
];
/// The downloaded copy is never executable, so scripts open in an editor; these don't need to be.
#[cfg(target_os = "linux")]
const RUN_WHEN_OPENED: &[&str] = &["desktop", "flatpakref", "jar"];

/// Whether `path`'s extension is one of `kinds` (lowercase, without the dot).
fn is_kind(path: &Path, kinds: &[impl AsRef<str>]) -> bool {
    let Some(extension) = path.extension().and_then(|e| e.to_str()) else { return false };
    kinds.iter().any(|kind| kind.as_ref().eq_ignore_ascii_case(extension))
}

/// Whether the system would run `path` rather than open it, if asked to open it.
fn runs_when_opened(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let listed = std::env::var("PATHEXT").unwrap_or_default();
        let listed: Vec<&str> = listed.split(';').map(|e| e.trim_start_matches('.')).filter(|e| !e.is_empty()).collect();
        if is_kind(path, &listed) {
            return true;
        }
    }
    is_kind(path, RUN_WHEN_OPENED)
}

/// Opens a local file in the editor from the settings, or the system's default application;
/// a file that application would run opens in the system's text editor instead (macOS,
/// Windows), or not at all until an editor is chosen (Linux, which has no such default).
/// Blocking: on macOS it waits for `open` to report whether the application was found.
pub fn open_in_editor(app: &AppHandle, editor: &str, path: &Path) -> Result<()> {
    let failed = |e: &dyn std::fmt::Display| Error::new("edit.openFailed").param("path", path.display()).detail(e);
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let mut open = std::process::Command::new("open");
        match editor {
            "" if runs_when_opened(path) => open.arg("-t"),
            "" => &mut open,
            editor => open.arg("-a").arg(editor),
        };
        let output = open.arg(path).output().map_err(|e| failed(&e))?;
        if !output.status.success() {
            return Err(failed(&String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        if editor.is_empty() && runs_when_opened(path) {
            #[cfg(windows)]
            return open_in_editor(app, "notepad.exe", path);
            #[cfg(target_os = "linux")]
            return Err(Error::new("edit.chooseEditor").param("name", path.file_name().unwrap_or_default().to_string_lossy()));
        }
        if editor.is_empty() {
            use tauri_plugin_opener::OpenerExt;
            return app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| failed(&e));
        }
        #[cfg(windows)]
        return std::process::Command::new(editor).arg(path).spawn().map(drop).map_err(|e| failed(&e));
        // Waited for, so that it doesn't stay a zombie once it exits.
        #[cfg(target_os = "linux")]
        {
            let mut child = std::process::Command::new(editor).arg(path).spawn().map_err(|e| failed(&e))?;
            std::thread::spawn(move || child.wait());
            Ok(())
        }
    }
}

/// The newest modification time of a folder and what is directly in it.
fn last_modified(dir: &Path) -> Option<SystemTime> {
    let own = std::fs::metadata(dir).and_then(|m| m.modified()).ok();
    let children = std::fs::read_dir(dir).ok()?.flatten().filter_map(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    children.chain(own).max()
}

/// Deletes the folders in `root` untouched for a while.
pub(crate) fn remove_old(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let old = last_modified(&path).is_some_and(|modified| SystemTime::now().duration_since(modified).is_ok_and(|age| age > KEEP_OLD));
        if old {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zshell-edit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Saves as editors that write a new file and rename it over the old one do.
    fn save_by_renaming(path: &Path, text: &str) {
        let staged = path.with_extension("swp");
        std::fs::write(&staged, text).unwrap();
        std::fs::rename(staged, path).unwrap();
    }

    #[tokio::test]
    async fn the_watcher_wakes_an_edit_when_its_folder_changes() {
        let dir = edit_folder();
        // As `watch` passes it: the temporary folder is behind a symlink on macOS, and the
        // system reports the real path.
        let folder = std::fs::canonicalize(&dir).unwrap();
        let watcher = Watcher::default();
        let wake = Arc::new(Notify::new());
        assert!(watcher.add(&folder, &wake));
        save_by_renaming(&dir.join("a.txt"), "saved");
        tokio::time::timeout(Duration::from_secs(10), wake.notified()).await.expect("woken by the save");
        watcher.remove(&folder, &wake);
        assert!(watcher.folders.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn each_save_is_reported_once() {
        let dir = edit_folder();
        let local = dir.join("a.txt");
        std::fs::write(&local, "downloaded").unwrap();
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
        let events = Channel::new(move |_| {
            let _ = sent.send(());
            Ok(())
        });
        let edits = Edits { root: dir.clone(), edits: Mutex::default(), watcher: Arc::default() };
        edits.start("main", "e1", "/remote/a.txt".into(), local.clone(), None, events);
        // Until the watch is set up: a save before it would be taken as what was downloaded.
        tokio::time::timeout(Duration::from_secs(10), async {
            while edits.watcher.folders.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the folder watched");

        save_by_renaming(&local, "first save");
        tokio::time::timeout(Duration::from_secs(10), received.recv()).await.expect("first save reported");
        std::fs::write(&local, "second save, written in place").unwrap();
        tokio::time::timeout(Duration::from_secs(10), received.recv()).await.expect("second save reported");
        tokio::time::sleep(SETTLE * 3).await;
        assert!(received.try_recv().is_err(), "a save reported twice");

        edits.stop("main", "e1");
        tokio::time::timeout(Duration::from_secs(10), async {
            while !edits.watcher.folders.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the folder unwatched once stopped");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn kinds_are_matched_by_extension_ignoring_case() {
        assert!(is_kind(Path::new("/tmp/a/Deploy.CMD"), &["cmd"]));
        assert!(!is_kind(Path::new("/tmp/a/cmd"), &["cmd"]));
        assert!(!is_kind(Path::new("/tmp/a/x.cmd.txt"), &["cmd"]));
    }

    #[test]
    fn files_the_system_would_run_are_told_apart() {
        #[cfg(target_os = "macos")]
        let runs = ["x.command", "x.webloc", "x.terminal"];
        #[cfg(windows)]
        let runs = ["x.bat", "x.lnk", "x.url"];
        #[cfg(target_os = "linux")]
        let runs = ["x.desktop", "x.jar", "x.flatpakref"];
        for name in runs {
            assert!(runs_when_opened(Path::new(name)), "{name}");
        }
        for name in ["x.txt", "x.sh", "x.conf", "Makefile"] {
            assert!(!runs_when_opened(Path::new(name)), "{name}");
        }
    }
}
