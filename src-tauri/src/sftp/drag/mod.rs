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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;

use super::transfer::{self, Progress, Reporter, Transfers};
use crate::error::{Error, Result};

/// A remote item being dragged.
#[derive(Clone, Deserialize, TS)]
#[ts(rename = "DragItem")]
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
/// file promise on macOS) and are downloaded one after another.
pub struct DropDownload {
    app: AppHandle,
    sftp: Arc<SftpSession>,
    transfer_id: String,
    events: Channel<DragEvent>,
    reporter: tokio::sync::Mutex<Reporter>,
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

/// What ending a drop (under the lock) leaves to do (outside it).
struct Ended {
    /// The items to report done; `None` when the drop failed.
    paths: Option<Vec<PathBuf>>,
}

impl DropDownload {
    fn new(app: &AppHandle, sftp: Arc<SftpSession>, transfer_id: String, count: usize, events: Channel<DragEvent>) -> Arc<Self> {
        let cancelled = app.state::<Transfers>().start(&transfer_id);
        let progress = events.clone();
        let reporter = Reporter::with_sink(move |p| drop(progress.send(DragEvent::Progress(p))), cancelled.clone());
        let state = State { pending: count, active: 0, last_activity: Instant::now(), failed: false, finished: false, paths: Vec::new() };
        Arc::new(Self {
            app: app.clone(),
            sftp,
            transfer_id,
            events,
            reporter: tokio::sync::Mutex::new(reporter),
            cancelled,
            count,
            state: Mutex::new(state),
        })
    }

    /// Downloads one item to `local`, replacing what is there; reports the end of the drop
    /// with the last item. After a failure, or once the drop is over, the remaining items are
    /// not downloaded.
    async fn fetch(&self, remote: &str, local: PathBuf) -> Result<()> {
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
            let mut reporter = self.reporter.lock().await;
            transfer::download_to(&self.sftp, &[(remote.to_owned(), local.clone())], &mut reporter).await.map_err(Error::from)
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
                Ok(()) => state.paths.push(local),
                Err(_) => {
                    // Once: the first failure is the drop's.
                    report = !state.failed && !state.finished;
                    state.failed = true;
                }
            }
            state.pending = state.pending.saturating_sub(1);
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
        self.app.state::<Transfers>().finish(&self.transfer_id);
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
    if result.outcome == Outcome::Outside {
        download.state.lock().unwrap().last_activity = Instant::now();
        download.watch();
    } else {
        download.app.state::<Transfers>().finish(&download.transfer_id);
    }
    Ok(result)
}
