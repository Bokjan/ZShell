//! Dragging remote files out of the window. Nothing is downloaded unless the items are
//! dropped on another application: macOS then asks for each through a file promise, with the
//! folder it was dropped in (`macos.rs`); on Windows and Linux they are downloaded into a
//! temporary folder once they have been dropped and the application they were dropped on asks
//! for them, and it copies them from there (`win.rs`, `linux.rs`).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;
use ts_rs::TS;

use super::transfer::{self, Left, Progress, Reporter, TransferSlots, Transfers, PROGRESS_INTERVAL};
use crate::error::{Error, Result};
use crate::local_name::local_file_name;

/// A remote item being dragged.
#[derive(Clone, Deserialize, TS)]
#[ts(rename = "DragItem")]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub path: String,
    pub is_dir: bool,
    /// Its permission bits (those of what a link points to), which its placeholder is made
    /// with, as a download's copy gets them; unknown for some servers. macOS makes none.
    #[serde(default)]
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub permissions: Option<u32>,
}

impl Item {
    fn name(&self) -> &str {
        super::file_name(&self.path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[ts(rename = "DragOutcome")]
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
#[derive(Clone, Copy, Debug, Serialize, TS)]
pub struct DragResult {
    pub outcome: Outcome,
    pub x: f64,
    pub y: f64,
}

impl DragResult {
    fn cancelled() -> Self {
        Self { outcome: Outcome::Cancelled, x: 0.0, y: 0.0 }
    }

    /// How a drag ended, from what the platform saw: whether the items were `dropped` at all,
    /// whether on this window (`inside`, released at `x`, `y` in the page), and whether the
    /// application they were dropped on `took` them (accepted the drop, or asked for the
    /// files). One dropped elsewhere that nobody took was refused, which is no drop.
    fn ended(dropped: bool, inside: bool, took: bool, x: f64, y: f64) -> Self {
        match (dropped, inside, took) {
            (true, true, _) => Self { outcome: Outcome::Inside, x, y },
            (true, false, true) => Self { outcome: Outcome::Outside, x: 0.0, y: 0.0 },
            _ => Self::cancelled(),
        }
    }
}

/// The download of a drop, reported to the frontend as one transfer.
#[derive(Clone, Serialize, TS)]
#[ts(rename = "DragOutEvent")]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DragEvent {
    Progress(Progress),
    Done { paths: Vec<PathBuf> },
    Error { error: Error },
}

/// After a drop on another application, how long it may go without asking for the items it
/// has not asked for yet before the drop is taken as over: the receiver may skip some (on
/// macOS each is asked for separately, and a name conflict can end the drop).
const IDLE_LIMIT: Duration = Duration::from_secs(300);

/// The downloads of one drop, shared by its items, which may be asked for separately (each
/// file promise on macOS) and download at once, within the connection's transfer slots.
pub struct DropDownload {
    app: AppHandle,
    sftp: Arc<SftpSession>,
    /// The webview that started the drag, which the transfer belongs to (see [`Transfers`]).
    webview: String,
    transfer_id: String,
    events: Channel<DragEvent>,
    /// The progress of all its items, which each download adds its own to.
    total: Arc<Mutex<Total>>,
    slots: Arc<TransferSlots>,
    cancelled: Arc<AtomicBool>,
    count: usize,
    /// One lock for all of it: an item starting to download and the drop ending (after the
    /// receiver went quiet) must not cross, or the item would download into a drop that has
    /// been reported done.
    state: Mutex<State>,
}

struct State {
    /// Items not yet downloaded (or failed).
    pending: usize,
    /// Items being downloaded now.
    active: usize,
    /// When an item was last asked for or done.
    last_activity: Instant,
    failed: bool,
    finished: bool,
    /// The items downloaded.
    paths: Vec<PathBuf>,
}

/// The progress of a drop's items together, reported as one transfer.
#[derive(Default)]
struct Total {
    progress: Progress,
    /// When it was last sent; `None` before the first time, which is at once.
    last_emit: Option<Instant>,
}

/// What ending a drop (under the lock) leaves to do (outside it).
struct Ended {
    /// The items to report done; `None` when the drop failed.
    paths: Option<Vec<PathBuf>>,
}

impl DropDownload {
    fn new(
        window: &WebviewWindow,
        sftp: Arc<SftpSession>,
        slots: Arc<TransferSlots>,
        transfer_id: String,
        count: usize,
        events: Channel<DragEvent>,
    ) -> Arc<Self> {
        let app = window.app_handle();
        let webview = window.label().to_owned();
        let cancelled = app.state::<Transfers>().start(&webview, &transfer_id);
        let state = State { pending: count, active: 0, last_activity: Instant::now(), failed: false, finished: false, paths: Vec::new() };
        Arc::new(Self {
            app: app.clone(),
            sftp,
            webview,
            transfer_id,
            events,
            total: Arc::default(),
            slots,
            cancelled,
            count,
            state: Mutex::new(state),
        })
    }

    /// A reporter for one download of the drop, which adds its progress to the drop's.
    fn reporter(&self) -> Reporter {
        let (total, events) = (self.total.clone(), self.events.clone());
        // What this download reported before: all of it counts up.
        let before = Mutex::new(Progress::default());
        let send = move |now: Progress| {
            let mut before = before.lock().unwrap();
            let mut total = total.lock().unwrap();
            let sum = &mut total.progress;
            sum.transferred += now.transferred - before.transferred;
            sum.total += now.total - before.total;
            sum.files_done += now.files_done - before.files_done;
            sum.files_total += now.files_total - before.files_total;
            sum.current.clone_from(&now.current);
            *before = now;
            if total.last_emit.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
                total.last_emit = Some(Instant::now());
                let _ = events.send(DragEvent::Progress(total.progress.clone()));
            }
        };
        Reporter::with_sink(send, self.cancelled.clone()).sharing(self.slots.clone())
    }

    /// Downloads items to their local paths, replacing what is there: all of them at once on
    /// Windows and Linux, each as it is asked for on macOS. Reports the end of the drop with
    /// the last item. After a failure, or once the drop is over, the remaining items are not
    /// downloaded.
    async fn fetch(&self, items: &[(String, PathBuf)]) -> Result<()> {
        let go = {
            let mut state = self.state.lock().unwrap();
            state.last_activity = Instant::now();
            let go = !state.failed && !state.finished;
            if go {
                state.active += 1;
            }
            go
        };
        let result = if go {
            transfer::download_to(&self.sftp, items, &mut self.reporter(), &mut Left::default()).await.map_err(Error::from)
        } else {
            Err(Error::new("transfer.cancelled"))
        };
        let (report, ended) = {
            let mut state = self.state.lock().unwrap();
            state.last_activity = Instant::now();
            if go {
                state.active -= 1;
            }
            let mut report = false;
            match &result {
                Ok(()) => state.paths.extend(items.iter().map(|(_, local)| local.clone())),
                Err(_) => {
                    // Once: the first failure is the drop's.
                    report = !state.failed && !state.finished;
                    state.failed = true;
                }
            }
            state.pending = state.pending.saturating_sub(items.len());
            let ended = (state.pending == 0).then(|| Self::end(&mut state)).flatten();
            (report, ended)
        };
        if let (true, Err(e)) = (report, &result) {
            let _ = self.events.send(DragEvent::Error { error: e.clone() });
        }
        if let Some(ended) = ended {
            self.ended(ended);
        }
        result
    }

    /// Lets go of the transfer's cancellation flag.
    fn finish(&self) {
        self.app.state::<Transfers>().finish(&self.webview, &self.transfer_id);
    }

    /// Ends the drop, once (`None` if it already had).
    fn end(state: &mut State) -> Option<Ended> {
        if std::mem::replace(&mut state.finished, true) {
            return None;
        }
        Some(Ended { paths: (!state.failed).then(|| std::mem::take(&mut state.paths)) })
    }

    /// What ending the drop does once the lock is let go: reports it done unless it failed,
    /// and lets go of what the platform kept for it.
    fn ended(&self, ended: Ended) {
        self.finish();
        if let Some(paths) = ended.paths {
            let _ = self.events.send(DragEvent::Done { paths });
        }
        #[cfg(target_os = "macos")]
        macos::release(self);
    }

    /// After a drop on another application: ends it when the transfer is cancelled, or when
    /// the receiver stops asking for the remaining items (`IDLE_LIMIT`), which otherwise
    /// would keep the transfer running and the connection referenced for good.
    fn watch(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                let (report, ended) = {
                    let mut state = self.state.lock().unwrap();
                    if state.finished {
                        return;
                    }
                    if state.active > 0 {
                        continue;
                    }
                    let idle = state.last_activity.elapsed() >= IDLE_LIMIT;
                    // A receiver that never asked for anything did not take the drop.
                    let untouched = state.pending == self.count;
                    let cancel = self.cancelled.load(Ordering::Relaxed) || (idle && untouched);
                    if !cancel && !idle {
                        continue;
                    }
                    let report = cancel && !state.failed;
                    if cancel {
                        state.failed = true;
                    }
                    (report, Self::end(&mut state))
                };
                if report {
                    let _ = self.events.send(DragEvent::Error { error: Error::new("transfer.cancelled") });
                }
                if let Some(ended) = ended {
                    self.ended(ended);
                }
                return;
            }
        });
    }
}

/// Makes the folder of a drop in `root` (`root/<transfer id>`, deleting old ones meanwhile) with
/// a placeholder for each item, in case the application dragged over looks at the files before
/// the drop; returns each remote item with where it goes. Each takes a free name: items the
/// file system can't tell apart (`README` and `readme` on Windows, or `a:b` and `a_b`, both
/// `a_b` there) would otherwise land on one file.
#[cfg(any(windows, target_os = "linux"))]
fn placeholders(root: &std::path::Path, transfer_id: &str, items: &[Item]) -> Result<Vec<(String, PathBuf)>> {
    use crate::local_name::{create_dir, create_unique, create_unique_file};

    // On a thread of its own: a large tree left from an earlier drop would hold up the drag,
    // with the mouse button down. This drop's folder is too new to go.
    let old = root.to_owned();
    std::thread::spawn(move || super::edit::remove_old(&old));
    if !super::is_folder_id(transfer_id) {
        return Err(Error::new("transfer.invalidPath").param("path", transfer_id));
    }
    let dir = root.join(transfer_id);
    std::fs::create_dir_all(&dir).map_err(|e| Error::new("transfer.createDirFailed").param("path", dir.display()).detail(e))?;
    items
        .iter()
        .map(|item| {
            let name = local_file_name(item.name()).ok_or_else(|| Error::new("transfer.invalidName").param("name", &item.path))?;
            let path = dir.join(name);
            let created = if item.is_dir {
                create_unique(&path, |path| create_dir(path, item.permissions.map(|mode| mode & 0o777 | 0o700), false))
            } else {
                create_unique_file(&path, item.permissions.map(|mode| mode & 0o777 | 0o200)).map(|(path, _)| (path, ()))
            };
            let (local, ()) = created.map_err(|e| Error::new("transfer.createFailed").param("path", path.display()).detail(e))?;
            Ok((item.path.clone(), local))
        })
        .collect()
}

/// Starts dragging `items` out of `window` (the mouse button must be down, pressed at
/// `pressed`) and waits for the drag to end. On Windows a window in the background that the
/// press brought forward goes back (`window::keep_behind`), so the drag can reach the window
/// that was in front of it; on macOS the press doesn't bring it forward (`window::macos`).
pub async fn drag_out(
    window: &WebviewWindow,
    sftp: Arc<SftpSession>,
    slots: Arc<TransferSlots>,
    transfer_id: String,
    items: Vec<Item>,
    pressed: Instant,
    events: Channel<DragEvent>,
) -> Result<DragResult> {
    if items.is_empty() {
        return Ok(DragResult::cancelled());
    }
    // Where it is dropped, an item is named after itself: one that can't name a local file
    // (`..`) isn't dragged, rather than left to the receiver to make safe (Finder on macOS).
    if let Some(item) = items.iter().find(|item| local_file_name(item.name()).is_none()) {
        return Err(Error::new("transfer.invalidName").param("name", &item.path));
    }
    let download = DropDownload::new(window, sftp, slots, transfer_id, items.len(), events);
    let (done, result) = oneshot::channel();
    #[cfg(target_os = "macos")]
    macos::start(window, items, download.clone(), done)?;
    #[cfg(windows)]
    win::start(window, items, download.clone(), pressed, done)?;
    #[cfg(target_os = "linux")]
    linux::start(window, items, download.clone(), done)?;
    #[cfg(not(windows))]
    let _ = pressed;
    let result = result.await.unwrap_or_else(|_| DragResult::cancelled());
    if result.outcome == Outcome::Outside {
        download.state.lock().unwrap().last_activity = Instant::now();
        download.watch();
    } else {
        download.finish();
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_counts_where_it_was_taken() {
        let ended = |dropped, inside, took| DragResult::ended(dropped, inside, took, 5.0, 6.0);
        let inside = ended(true, true, false);
        assert_eq!((inside.outcome, inside.x, inside.y), (Outcome::Inside, 5.0, 6.0));
        assert_eq!(ended(true, false, true).outcome, Outcome::Outside);
        // Refused by the window it was dropped on, or not dropped.
        assert_eq!(ended(true, false, false).outcome, Outcome::Cancelled);
        assert_eq!(ended(false, true, true).outcome, Outcome::Cancelled);
        assert_eq!(ended(false, false, true).outcome, Outcome::Cancelled);
    }
}
