//! Commands for the file panel: SFTP operations, transfers, editing in another app and
//! dragging files out.

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::error::{Error, Result};
use crate::session::SessionId;
use crate::settings::SettingsStore;
use crate::sftp::drag;
use crate::sftp::edit::{self, EditEvent, Edits};
use crate::sftp::{self, transfer, Listing};
use crate::ssh::Connections;

use super::blocking;

/// Opens (or reuses) the session's SFTP channel and returns the remote home directory.
#[tauri::command]
pub async fn sftp_open(connections: State<'_, Connections>, id: SessionId) -> Result<String> {
    let sftp = connections.get(id)?.sftp().await?;
    sftp.canonicalize(".").await.map_err(|e| Error::new("sftp.listFailed").param("path", "~").detail(e))
}

#[tauri::command]
pub async fn sftp_list(connections: State<'_, Connections>, id: SessionId, path: String) -> Result<Listing> {
    let sftp = connections.get(id)?.sftp().await?;
    sftp::list(&sftp, &path).await.map_err(|e| Error::from(e.context(Error::new("sftp.listFailed").param("path", &path))))
}

#[tauri::command]
pub async fn sftp_mkdir(connections: State<'_, Connections>, id: SessionId, path: String) -> Result<()> {
    let sftp = connections.get(id)?.sftp().await?;
    sftp.create_dir(&path).await.map_err(|e| Error::new("sftp.mkdirFailed").param("path", &path).detail(e))
}

#[tauri::command]
pub async fn sftp_rename(connections: State<'_, Connections>, id: SessionId, from: String, to: String) -> Result<()> {
    let sftp = connections.get(id)?.sftp().await?;
    sftp.rename(&from, &to)
        .await
        .map_err(|e| Error::new("sftp.renameFailed").param("from", &from).param("to", &to).detail(e))
}

#[tauri::command]
pub async fn sftp_remove(connections: State<'_, Connections>, id: SessionId, path: String) -> Result<()> {
    let sftp = connections.get(id)?.sftp().await?;
    sftp::remove(&sftp, &path).await.map_err(|e| Error::from(e.context(Error::new("sftp.removeFailed").param("path", &path))))
}

#[tauri::command]
pub async fn sftp_chmod(connections: State<'_, Connections>, id: SessionId, path: String, mode: u32) -> Result<()> {
    let sftp = connections.get(id)?.sftp().await?;
    sftp::chmod(&sftp, &path, mode).await.map_err(|e| Error::from(e.context(Error::new("sftp.chmodFailed").param("path", &path))))
}

#[tauri::command]
pub async fn sftp_upload(
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    local_paths: Vec<PathBuf>,
    remote_dir: String,
    on_progress: Channel<transfer::Progress>,
) -> Result<()> {
    let sftp = connections.get(id)?.transfer_sftp().await?;
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&transfer_id));
    let result = transfer::upload(&sftp, &local_paths, &remote_dir, &mut reporter).await;
    transfers.finish(&transfer_id);
    Ok(result?)
}

/// Downloads into `local_dir` (default: the download folder from the settings); returns the
/// created paths.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    settings: State<'_, SettingsStore>,
    id: SessionId,
    transfer_id: String,
    remote_paths: Vec<String>,
    local_dir: Option<PathBuf>,
    on_progress: Channel<transfer::Progress>,
) -> Result<Vec<PathBuf>> {
    let sftp = connections.get(id)?.transfer_sftp().await?;
    let local_dir = match local_dir {
        Some(dir) => dir,
        None => download_dir(&app, &settings)?,
    };
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&transfer_id));
    let result = transfer::download(&sftp, &remote_paths, &local_dir, &mut reporter).await;
    transfers.finish(&transfer_id);
    Ok(result?)
}

/// Downloads one item to `local_path`, replacing what is there (the save dialog asked).
#[tauri::command]
pub async fn sftp_download_as(
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    remote_path: String,
    local_path: PathBuf,
    on_progress: Channel<transfer::Progress>,
) -> Result<PathBuf> {
    let sftp = connections.get(id)?.transfer_sftp().await?;
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&transfer_id));
    let result = transfer::download_to(&sftp, &[(remote_path, local_path.clone())], &mut reporter).await;
    transfers.finish(&transfer_id);
    result?;
    Ok(local_path)
}

/// Where downloads go without asking: the folder chosen in the settings, else Downloads.
pub(super) fn download_dir(app: &AppHandle, settings: &SettingsStore) -> Result<PathBuf> {
    let chosen = settings.get().files.download_directory;
    if chosen.is_empty() {
        return Ok(app.path().download_dir()?);
    }
    let dir = PathBuf::from(chosen);
    std::fs::create_dir_all(&dir).map_err(|e| Error::new("transfer.createDirFailed").param("path", dir.display()).detail(e))?;
    Ok(dir)
}

/// The folder downloads go to without asking, for the settings.
#[tauri::command]
pub async fn downloads_directory(app: AppHandle) -> Result<PathBuf> {
    blocking(app, |app| download_dir(app, &app.state::<SettingsStore>())).await
}

/// Downloads a remote file into a temporary folder, opens it in the editor and watches it;
/// `on_event` reports each save. Returns the local copy.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_edit_open(
    app: AppHandle,
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
    let sftp = connections.get(id)?.sftp().await?;
    let local = edits.local_path(&edit_id, &remote_path)?;
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&edit_id));
    let result = transfer::download_to(&sftp, &[(remote_path.clone(), local.clone())], &mut reporter).await;
    transfers.finish(&edit_id);
    result?;
    edits.start(&sftp, edit_id.clone(), remote_path, local.clone(), on_event).await;
    if let Err(e) = open_editor(&app, &settings, local.clone()).await {
        edits.stop(&edit_id);
        return Err(e);
    }
    Ok(local)
}

/// Opens a file being edited again.
#[tauri::command]
pub async fn sftp_edit_reopen(app: AppHandle, edits: State<'_, Edits>, settings: State<'_, SettingsStore>, edit_id: String) -> Result<()> {
    open_editor(&app, &settings, edits.local(&edit_id)?).await
}

async fn open_editor(app: &AppHandle, settings: &SettingsStore, path: PathBuf) -> Result<()> {
    let app = app.clone();
    let editor = settings.get().files.editor;
    tauri::async_runtime::spawn_blocking(move || edit::open_in_editor(&app, &editor, &path))
        .await
        .map_err(|e| Error::new("unexpected").detail(e))?
}

/// Uploads a saved file being edited; fails with `edit.conflict` unless `force` when the
/// remote file changed meanwhile.
#[tauri::command]
pub async fn sftp_edit_upload(
    connections: State<'_, Connections>,
    edits: State<'_, Edits>,
    id: SessionId,
    edit_id: String,
    force: bool,
) -> Result<()> {
    let sftp = connections.get(id)?.sftp().await?;
    edits.upload(&sftp, &edit_id, force).await
}

#[tauri::command]
pub fn sftp_edit_stop(edits: State<'_, Edits>, edit_id: String) {
    edits.stop(&edit_id);
}

/// Drags remote items out of the window (the mouse button must be down) and returns how the
/// drag ended; items dropped on another application are downloaded, reported through
/// `on_event`.
#[tauri::command]
pub async fn sftp_drag_out(
    window: WebviewWindow,
    connections: State<'_, Connections>,
    id: SessionId,
    transfer_id: String,
    items: Vec<drag::Item>,
    on_event: Channel<drag::DragEvent>,
) -> Result<drag::DragResult> {
    let sftp = connections.get(id)?.transfer_sftp().await?;
    drag::drag_out(&window, sftp, transfer_id, items, on_event).await
}

#[tauri::command]
pub fn transfer_cancel(transfers: State<'_, transfer::Transfers>, transfer_id: String) {
    transfers.cancel(&transfer_id);
}
