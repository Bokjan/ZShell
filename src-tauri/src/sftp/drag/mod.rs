//! Dragging remote files out of the window. Nothing is downloaded unless the items are
//! dropped on another application: macOS then asks for each through a file promise, with the
//! folder it was dropped in (`macos.rs`); on Windows they are downloaded into a temporary
//! folder when the mouse button is released and the application they were dropped on copies
//! them from there (`win.rs`).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;

use super::transfer::{self, Progress, Reporter, Transfers};
use crate::error::{Error, Result};

/// A remote item being dragged.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub path: String,
    pub is_dir: bool,
}

impl Item {
    fn name(&self) -> &str {
        super::file_name(&self.path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Cancelled,
    /// Dropped on this window; the frontend decides what that means from the position.
    Inside,
    /// Dropped on another application, which has the items downloaded.
    Outside,
}

/// How a drag ended, with where the mouse was released (in the page's CSS pixels) for drops
/// on this window.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct DragResult {
    pub outcome: Outcome,
    pub x: f64,
    pub y: f64,
}

impl DragResult {
    fn cancelled() -> Self {
        Self { outcome: Outcome::Cancelled, x: 0.0, y: 0.0 }
    }
}

/// The download of a drop, reported to the frontend as one transfer.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DragEvent {
    Progress(Progress),
    Done { paths: Vec<PathBuf> },
    Error { error: Error },
}

/// The downloads of one drop, shared by its items, which may be asked for separately (each
/// file promise on macOS) and are downloaded one after another.
pub struct DropDownload {
    app: AppHandle,
    sftp: Arc<SftpSession>,
    transfer_id: String,
    events: Channel<DragEvent>,
    reporter: tokio::sync::Mutex<Reporter>,
    /// Items not yet downloaded (or failed).
    pending: AtomicUsize,
    failed: AtomicBool,
    paths: Mutex<Vec<PathBuf>>,
}

impl DropDownload {
    fn new(app: &AppHandle, sftp: Arc<SftpSession>, transfer_id: String, count: usize, events: Channel<DragEvent>) -> Arc<Self> {
        let cancelled = app.state::<Transfers>().start(&transfer_id);
        let progress = events.clone();
        let reporter = Reporter::with_sink(move |p| drop(progress.send(DragEvent::Progress(p))), cancelled);
        Arc::new(Self {
            app: app.clone(),
            sftp,
            transfer_id,
            events,
            reporter: tokio::sync::Mutex::new(reporter),
            pending: AtomicUsize::new(count),
            failed: AtomicBool::new(false),
            paths: Mutex::new(Vec::new()),
        })
    }

    /// Downloads one item to `local`, replacing what is there; reports the end of the drop
    /// with the last item. After a failure the remaining items are not downloaded.
    async fn fetch(&self, remote: &str, local: PathBuf) -> Result<()> {
        let result = if self.failed.load(Ordering::Relaxed) {
            Err(Error::new("transfer.cancelled"))
        } else {
            let mut reporter = self.reporter.lock().await;
            transfer::download_to(&self.sftp, &[(remote.to_owned(), local.clone())], &mut reporter).await.map_err(Error::from)
        };
        match &result {
            Ok(()) => self.paths.lock().unwrap().push(local),
            Err(e) => {
                if !self.failed.swap(true, Ordering::Relaxed) {
                    let _ = self.events.send(DragEvent::Error { error: e.clone() });
                }
            }
        }
        if self.pending.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.finish();
        }
        result
    }

    /// Ends a drop whose items will not all be asked for (or none at all).
    fn finish(&self) {
        self.app.state::<Transfers>().finish(&self.transfer_id);
        if !self.failed.load(Ordering::Relaxed) {
            let paths = std::mem::take(&mut *self.paths.lock().unwrap());
            let _ = self.events.send(DragEvent::Done { paths });
        }
    }
}

/// Starts dragging `items` out of `window` (the mouse button must be down) and waits for the
/// drag to end.
pub async fn drag_out(
    window: &WebviewWindow,
    sftp: Arc<SftpSession>,
    transfer_id: String,
    items: Vec<Item>,
    events: Channel<DragEvent>,
) -> Result<DragResult> {
    if items.is_empty() {
        return Ok(DragResult::cancelled());
    }
    let download = DropDownload::new(window.app_handle(), sftp, transfer_id, items.len(), events);
    let (done, result) = oneshot::channel();
    #[cfg(target_os = "macos")]
    macos::start(window, items, download.clone(), done)?;
    #[cfg(windows)]
    win::start(window, items, download.clone(), done)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    drop((items, done));
    let result = result.await.unwrap_or_else(|_| DragResult::cancelled());
    if result.outcome != Outcome::Outside {
        download.app.state::<Transfers>().finish(&download.transfer_id);
    }
    Ok(result)
}
