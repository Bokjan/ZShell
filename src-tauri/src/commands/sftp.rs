//! Commands for the file panel: SFTP operations, transfers, editing in another app and
//! dragging files out.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::ipc::Channel;

use tauri::{AppHandle, Manager, State, Webview, WebviewWindow};

use crate::connections::{Connections, LiveConnection};
use crate::error::{Error, Result};
use crate::session::{SessionId, SessionManager};
use crate::settings::SettingsStore;
use crate::sftp::drag;
use crate::sftp::edit::{self, EditEvent, Edits};
use crate::sftp::{self, transfer, Listing};

use super::blocking;

/// Session `id`'s SSH connection, for a file panel of `webview`: its own session's (see
/// [`SessionManager::check`]).
fn connection(webview: &str, sessions: &SessionManager, connections: &Connections, id: SessionId) -> Result<Arc<LiveConnection>> {
    sessions.check(webview, id)?;
    connections.get(id)
}

/// Opens (or reuses) the session's SFTP channel and returns the remote home directory.
#[tauri::command]
pub async fn sftp_open(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
) -> Result<String> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    sftp.canonicalize(".").await.map_err(|e| Error::new("sftp.listFailed").param("path", "~").detail(e))
}

/// The folder the session's shell is in, where the server lets it be found (Linux); `None`
/// elsewhere.
#[tauri::command]
pub async fn sftp_shell_dir(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
) -> Result<Option<String>> {
    connection(webview.label(), &sessions, &connections, id)?.shell_dir(id).await.map_err(Error::from)
}

#[tauri::command]
pub async fn sftp_list(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    path: String,
) -> Result<Listing> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    sftp::list(&sftp, &path).await.map_err(|e| Error::from(e.context(Error::new("sftp.listFailed").param("path", &path))))
}

#[tauri::command]
pub async fn sftp_mkdir(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    path: String,
) -> Result<()> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    sftp.create_dir(&path).await.map_err(|e| Error::new("sftp.mkdirFailed").param("path", &path).detail(e))
}

#[tauri::command]
pub async fn sftp_rename(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    from: String,
    to: String,
) -> Result<()> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    sftp.rename(&from, &to).await.map_err(|e| Error::new("sftp.renameFailed").param("from", &from).param("to", &to).detail(e))
}

#[tauri::command]
pub async fn sftp_remove(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    path: String,
) -> Result<()> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    sftp::remove(&sftp, &path).await.map_err(|e| Error::from(e.context(Error::new("sftp.removeFailed").param("path", &path))))
}

#[tauri::command]
pub async fn sftp_chmod(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    path: String,
    mode: u32,
) -> Result<()> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    sftp::chmod(&sftp, &path, mode).await.map_err(|e| Error::from(e.context(Error::new("sftp.chmodFailed").param("path", &path))))
}

/// Uploads into `remote_dir`. Ends with what it created on the server, kept for
/// [`transfer_delete`], also when it fails.
#[tauri::command]
pub async fn sftp_upload(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    local_paths: Vec<PathBuf>,
    remote_dir: String,
    on_progress: Channel<transfer::Progress>,
) -> Result<transfer::Ended> {
    let cancelled = transfers.start(webview.label(), &transfer_id);
    let mut left = transfer::Left::default();
    let result = async {
        let connection = connection(webview.label(), &sessions, &connections, id)?;
        let sftp = connection.transfer_sftp().await?;
        let mut reporter = transfer::Reporter::new(on_progress, cancelled).sharing(connection.transfer_slots());
        Ok(transfer::upload(&sftp, &local_paths, &remote_dir, &mut reporter, &mut left).await?)
    };
    let result: Result<()> = result.await;
    transfers.finish(webview.label(), &transfer_id);
    let ended = transfer::Ended::new(&left, result.err(), Vec::new());
    transfers.keep(webview.label(), &transfer_id, transfer::Kept::Remote(left));
    Ok(ended)
}

/// Downloads into `local_dir` (default: the download folder from the settings). Ends with the
/// top-level items it created, and keeps all it created for [`transfer_delete`], also when it
/// fails.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    remote_paths: Vec<String>,
    local_dir: Option<PathBuf>,
    on_progress: Channel<transfer::Progress>,
) -> Result<transfer::Ended> {
    let cancelled = transfers.start(webview.label(), &transfer_id);
    let mut left = transfer::Left::default();
    let result = async {
        let connection = connection(webview.label(), &sessions, &connections, id)?;
        let sftp = connection.transfer_sftp().await?;
        let local_dir = match local_dir {
            Some(dir) => dir,
            // Made if it isn't there: a disk that may be slow to answer.
            None => blocking(app, |app| app.state::<SettingsStore>().download_dir(app)).await?,
        };
        let mut reporter = transfer::Reporter::new(on_progress, cancelled).sharing(connection.transfer_slots());
        Ok(transfer::download(&sftp, &remote_paths, &local_dir, &mut reporter, &mut left).await?)
    };
    let result: Result<()> = result.await;
    transfers.finish(webview.label(), &transfer_id);
    let paths = std::mem::take(&mut left.items);
    let ended = transfer::Ended::new(&left, result.err(), paths);
    transfers.keep(webview.label(), &transfer_id, transfer::Kept::Local(left));
    Ok(ended)
}

/// Downloads one item to `local_path`, replacing what is there (the save dialog asked). Ends
/// like [`sftp_download`].
#[tauri::command]
pub async fn sftp_download_as(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    remote_path: String,
    local_path: PathBuf,
    on_progress: Channel<transfer::Progress>,
) -> Result<transfer::Ended> {
    let cancelled = transfers.start(webview.label(), &transfer_id);
    let mut left = transfer::Left::default();
    let result = async {
        let connection = connection(webview.label(), &sessions, &connections, id)?;
        let sftp = connection.transfer_sftp().await?;
        let mut reporter = transfer::Reporter::new(on_progress, cancelled).sharing(connection.transfer_slots());
        Ok(transfer::download_to(&sftp, &[(remote_path, local_path.clone())], &mut reporter, &mut left).await?)
    };
    let result: Result<()> = result.await;
    transfers.finish(webview.label(), &transfer_id);
    let paths = if result.is_ok() { vec![local_path] } else { Vec::new() };
    let ended = transfer::Ended::new(&left, result.err(), paths);
    transfers.keep(webview.label(), &transfer_id, transfer::Kept::Local(left));
    Ok(ended)
}

/// The folder downloads go to without asking, for the settings.
#[tauri::command]
pub async fn downloads_directory(app: AppHandle) -> Result<PathBuf> {
    blocking(app, |app| app.state::<SettingsStore>().download_dir(app)).await
}

/// Downloads a remote file into a temporary folder, opens it in the editor and watches it;
/// `on_event` reports each save. Returns the local copy.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_edit_open(
    app: AppHandle,
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    edits: State<'_, Edits>,
    settings: State<'_, SettingsStore>,
    id: SessionId,
    edit_id: String,
    remote_path: String,
    on_progress: Channel<transfer::Progress>,
    on_event: Channel<EditEvent>,
) -> Result<PathBuf> {
    let cancelled = transfers.start(webview.label(), &edit_id);
    let result = async {
        let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
        let local = edits.local_path(&edit_id, &remote_path)?;
        let remote_stamp = edit::remote_stamp(&sftp, &remote_path).await;
        let mut reporter = transfer::Reporter::new(on_progress, cancelled.clone());
        transfer::download_to(&sftp, &[(remote_path.clone(), local.clone())], &mut reporter, &mut transfer::Left::default()).await?;
        edits.start(webview.label(), &edit_id, remote_path, local.clone(), remote_stamp, on_event);
        Result::Ok(local)
    };
    // Watched while the download still counts as the page's: if the page went since, the
    // download was cancelled with it, and the edit, watched too late for the page to stop it,
    // stops here.
    let result = result.await.and_then(|local| {
        if !cancelled.load(Ordering::Relaxed) {
            return Ok(local);
        }
        edits.stop(webview.label(), &edit_id);
        Err(Error::new("transfer.cancelled"))
    });
    transfers.finish(webview.label(), &edit_id);
    let local = result?;
    if let Err(e) = open_editor(&app, &settings, local.clone()).await {
        edits.stop(webview.label(), &edit_id);
        return Err(e);
    }
    Ok(local)
}

/// Opens a file being edited again.
#[tauri::command]
pub async fn sftp_edit_reopen(
    app: AppHandle,
    webview: Webview,
    edits: State<'_, Edits>,
    settings: State<'_, SettingsStore>,
    edit_id: String,
) -> Result<()> {
    open_editor(&app, &settings, edits.local(webview.label(), &edit_id)?).await
}

async fn open_editor(app: &AppHandle, settings: &SettingsStore, path: PathBuf) -> Result<()> {
    let app = app.clone();
    let editor = settings.get().files.editor;
    tauri::async_runtime::spawn_blocking(move || edit::open_in_editor(&app, &editor, &path))
        .await
        .map_err(|e| Error::new("unexpected").detail(e))?
}

/// Uploads a saved file being edited, through session `id`'s connection (the tab's current
/// one, which may not be the one it was downloaded through); fails with `edit.conflict`
/// unless `force` when the remote file changed meanwhile.
#[tauri::command]
pub async fn sftp_edit_upload(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    edits: State<'_, Edits>,
    id: SessionId,
    edit_id: String,
    force: bool,
) -> Result<()> {
    let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
    edits.upload(&sftp, webview.label(), &edit_id, force).await
}

#[tauri::command]
pub fn sftp_edit_stop(webview: Webview, edits: State<'_, Edits>, edit_id: String) {
    edits.stop(webview.label(), &edit_id);
}

/// Drags remote items out of the window (the mouse button must be down) and returns how the
/// drag ended; items dropped on another application are downloaded, reported through
/// `on_event`.
#[tauri::command]
pub async fn sftp_drag_out(
    window: WebviewWindow,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    id: SessionId,
    transfer_id: String,
    items: Vec<drag::Item>,
    since_press: u32,
    on_event: Channel<drag::DragEvent>,
) -> Result<drag::DragResult> {
    // Before waiting for the SFTP channel.
    let pressed = Instant::now().checked_sub(Duration::from_millis(since_press.into())).unwrap_or_else(Instant::now);
    let connection = connection(window.label(), &sessions, &connections, id)?;
    let sftp = connection.transfer_sftp().await?;
    drag::drag_out(&window, sftp, connection.transfer_slots(), transfer_id, items, pressed, on_event).await
}

#[tauri::command]
pub fn transfer_cancel(webview: Webview, transfers: State<'_, transfer::Transfers>, transfer_id: String) {
    transfers.cancel(webview.label(), &transfer_id);
}

/// Permanently deletes what an ended transfer created (see `transfer::left`): on the server
/// through session `id`'s connection for an upload. Returns how many files were deleted, and
/// how many were kept as they changed since; on a failure, what is left can be deleted again.
#[tauri::command]
pub async fn transfer_delete(
    app: AppHandle,
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: Option<SessionId>,
    transfer_id: String,
) -> Result<transfer::Deletion> {
    let label = webview.label();
    let Some(kept) = transfers.take(label, &transfer_id) else { return Ok(transfer::Deletion::default()) };
    let (kept, (deletion, result)) = match kept {
        transfer::Kept::Local(mut left) => {
            let (left, outcome) = blocking(app, move |_| {
                let outcome = transfer::delete_local(&mut left);
                Ok((left, outcome))
            })
            .await?;
            (transfer::Kept::Local(left), outcome)
        }
        transfer::Kept::Remote(mut left) => {
            let sftp =
                async { connection(label, &sessions, &connections, id.ok_or_else(|| Error::new("session.notConnected"))?)?.sftp().await };
            let outcome = match sftp.await {
                Ok(sftp) => transfer::delete_remote(&sftp, &mut left).await,
                Err(e) => (transfer::Deletion::default(), Err(Error::from(e))),
            };
            (transfer::Kept::Remote(left), outcome)
        }
    };
    transfers.keep(label, &transfer_id, kept);
    result.map(|()| deletion)
}

/// Forgets what the transfers `transfer_ids` left, which the file panel no longer lists.
#[tauri::command]
pub fn transfer_forget(webview: Webview, transfers: State<'_, transfer::Transfers>, transfer_ids: Vec<String>) {
    transfers.forget(webview.label(), &transfer_ids);
}
