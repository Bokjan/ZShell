//! Editing remote files in a local editor: the file is downloaded into a temporary folder and
//! opened, then watched; each save is reported to the frontend, which uploads it (through the
//! tab's current connection, so editing survives reconnects).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use russh_sftp::client::SftpSession;
use serde::Serialize;
use ts_rs::TS;
use tauri::ipc::Channel;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

use super::file_name;
use super::replace::RemoteTarget;
use crate::local_name::local_file_name;
use crate::error::{Error, Result};

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
    stopped: Arc<AtomicBool>,
}

/// Files being edited, keyed by a frontend-chosen id.
pub struct Edits {
    root: PathBuf,
    edits: Mutex<HashMap<String, Arc<Edit>>>,
}

impl Edits {
    pub fn new(root: PathBuf) -> Self {
        remove_old(&root);
        Self { root, edits: Mutex::new(HashMap::new()) }
    }

    /// Where `remote` is downloaded to for editing, in a folder of its own so the editor
    /// shows the real file name.
    pub fn local_path(&self, edit_id: &str, remote: &str) -> Result<PathBuf> {
        let Some(name) = local_file_name(file_name(remote)) else {
            return Err(Error::new("transfer.invalidName").param("name", remote));
        };
        if edit_id.contains(['/', '\\', '.']) {
            return Err(Error::new("transfer.invalidPath").param("path", remote));
        }
        Ok(self.root.join(edit_id).join(name))
    }

    /// Starts watching a file downloaded to `local`, reporting saves through `events`.
    /// `remote_stamp`: the remote file's, taken before downloading it (see `remote_stamp`).
    pub fn start(&self, edit_id: String, remote: String, local: PathBuf, remote_stamp: Option<Stamp>, events: Channel<EditEvent>) {
        let stopped = Arc::new(AtomicBool::new(false));
        let edit = Arc::new(Edit {
            remote,
            local: local.clone(),
            remote_stamp: Mutex::new(remote_stamp),
            uploading: tokio::sync::Mutex::new(()),
            stopped: stopped.clone(),
        });
        if let Some(old) = self.edits.lock().unwrap().insert(edit_id, edit) {
            old.stopped.store(true, Ordering::Relaxed);
        }
        tauri::async_runtime::spawn(watch(local, stopped, events));
    }

    fn get(&self, edit_id: &str) -> Result<Arc<Edit>> {
        self.edits.lock().unwrap().get(edit_id).cloned().ok_or_else(|| Error::new("edit.notFound"))
    }

    pub fn local(&self, edit_id: &str) -> Result<PathBuf> {
        Ok(self.get(edit_id)?.local.clone())
    }

    /// Uploads the local copy over the remote file. Unless `force`, fails with
    /// `edit.conflict` when the remote file changed since it was downloaded or last uploaded.
    pub async fn upload(&self, sftp: &SftpSession, edit_id: &str, force: bool) -> Result<()> {
        let edit = self.get(edit_id)?;
        let _uploading = edit.uploading.lock().await;
        let remote = &edit.remote;
        let expected = *edit.remote_stamp.lock().unwrap();
        if !force && remote_stamp(sftp, remote).await != expected {
            return Err(Error::new("edit.conflict").param("name", file_name(remote)));
        }
        let contents = tokio::fs::read(&edit.local)
            .await
            .map_err(|e| Error::new("transfer.readFailed").param("path", edit.local.display()).detail(e))?;
        let result: anyhow::Result<()> = async {
            // Replaced only once complete, keeping its owner and mode (see `replace`).
            let mut target = RemoteTarget::create(sftp, remote).await?;
            let written: anyhow::Result<()> = async {
                target.file.write_all(&contents).await?;
                target.file.flush().await?;
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
    pub fn stop(&self, edit_id: &str) {
        if let Some(edit) = self.edits.lock().unwrap().remove(edit_id) {
            edit.stopped.store(true, Ordering::Relaxed);
        }
    }
}

/// The remote file's size and modification time, which tell whether it changed since. Taken
/// before downloading it for editing: a change made during the download then shows as one.
pub async fn remote_stamp(sftp: &SftpSession, remote: &str) -> Option<Stamp> {
    let metadata = sftp.metadata(remote).await.ok()?;
    Some(Stamp { size: metadata.size.unwrap_or(0), modified: metadata.mtime })
}

/// Polls the local copy, reporting a change once it has stayed the same for one interval, so
/// a save still being written is not uploaded half-way.
async fn watch(local: PathBuf, stopped: Arc<AtomicBool>, events: Channel<EditEvent>) {
    let stamp = |metadata: std::fs::Metadata| (metadata.len(), metadata.modified().ok());
    let mut reported = tokio::fs::metadata(&local).await.ok().map(stamp);
    let mut previous = reported;
    while !stopped.load(Ordering::Relaxed) {
        tokio::time::sleep(POLL_INTERVAL).await;
        // Missing for a moment while editors that save by renaming replace it.
        let Ok(metadata) = tokio::fs::metadata(&local).await else { continue };
        let current = Some(stamp(metadata));
        if current == previous && current != reported && !stopped.load(Ordering::Relaxed) {
            reported = current;
            if events.send(EditEvent::Changed).is_err() {
                break;
            }
        }
        previous = current;
    }
}

/// Opens a local file in the editor from the settings, or the system's default application.
/// Blocking: on macOS it waits for `open` to report whether the application was found.
pub fn open_in_editor(app: &AppHandle, editor: &str, path: &Path) -> Result<()> {
    let failed = |e: &dyn std::fmt::Display| Error::new("edit.openFailed").param("path", path.display()).detail(e);
    if editor.is_empty() {
        use tauri_plugin_opener::OpenerExt;
        return app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| failed(&e));
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("open").arg("-a").arg(editor).arg(path).output().map_err(|e| failed(&e))?;
        if !output.status.success() {
            return Err(failed(&String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    std::process::Command::new(editor).arg(path).spawn().map(drop).map_err(|e| failed(&e))
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
        let old = last_modified(&path)
            .is_some_and(|modified| SystemTime::now().duration_since(modified).is_ok_and(|age| age > KEEP_OLD));
        if old {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
