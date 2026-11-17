//! Commands for the file panel: SFTP operations, transfers, editing in another app and
//! dragging files out.

use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    sftp::list(&sftp, &path).await.map_err(|e| {
        // Told apart, so that the panel forgets a folder gone since (one gone back to, say).
        let code = if sftp::is_gone(&e) { "sftp.notFound" } else { "sftp.listFailed" };
        Error::from(e.context(Error::new(code).param("path", &path)))
    })
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

/// Runs `job` through session `id`'s connection, as `webview`'s transfer `transfer_id`, or goes
/// on with it (`going_on`: whether to check the files copied whole before, see
/// [`transfer::Job::run`]). A transfer that fails is kept to go on with (see
/// `transfer::resume`), unless cancelled; what it created is kept for [`transfer_delete`],
/// whichever way it ends.
#[allow(clippy::too_many_arguments)]
async fn run_transfer(
    webview: &str,
    sessions: &SessionManager,
    connections: &Connections,
    transfers: &transfer::Transfers,
    id: SessionId,
    transfer_id: &str,
    mut job: transfer::Job,
    going_on: Option<bool>,
    on_progress: Channel<transfer::Progress>,
) -> transfer::Ended {
    // Until it has removed what it leaves incomplete, which quitting waits for.
    let _cleaning = crate::cleanup::hold();
    let cancelled = transfers.start(webview, transfer_id);
    let token = cancelled.clone();
    let mut left = going_on.and_then(|_| transfers.take_left(webview, transfer_id)).unwrap_or_else(|| job.nothing_left());
    let (mut ssh, mut sftp) = (None, None);
    let result = async {
        let connection = connection(webview, sessions, connections, id)?;
        ssh = Some(connection.ssh().clone());
        // An upload is gone on with on the server it went to only: the tab may have connected
        // elsewhere since (its session edited, then connected again).
        if let Some(server) = job.server() {
            server.check_same(&transfer::Server::of(&connection.ssh().profile().ssh.remote), "transfer.resumeElsewhere")?;
        }
        let opened = connection.transfer_sftp().await?;
        sftp = Some(opened.clone());
        let mut reporter = transfer::Reporter::new(on_progress, cancelled).sharing(connection.transfer_slots());
        job.run(&opened, &mut reporter, &mut left, going_on == Some(true)).await
    };
    let mut result: anyhow::Result<()> = result.await;
    let paths = job.paths(&mut left, result.is_ok());
    // Cancelled as it failed otherwise (its connection lost, say): cancelled all the same.
    let cancelled_too = |result: &mut anyhow::Result<()>| {
        if result.as_ref().is_err_and(|e| !transfer::is_cancelled(e)) {
            *result = Err(Error::new("transfer.cancelled").into());
        }
    };
    if token.is_cancelled() {
        cancelled_too(&mut result);
    }
    let mut resume = match &result {
        Err(e) if !transfer::is_cancelled(e) => Some(match &ssh {
            Some(ssh) if transfer::ended_by_loss(ssh, e).await => transfer::Resume::Lost,
            _ => transfer::Resume::Retry,
        }),
        _ => None,
    };
    let discarded = match resume {
        Some(_) => transfers.pause(webview, transfer_id, job, &token),
        None if result.is_err() => Some(job),
        None => None,
    };
    if let Some(job) = discarded {
        if resume.take().is_some() {
            cancelled_too(&mut result);
        }
        job.discard(sftp.as_deref()).await;
    }
    transfers.end(webview, transfer_id, left, result.map_err(Error::from), paths, resume)
}

/// Uploads into `remote_dir`. Ends with what it created on the server, kept for
/// [`transfer_delete`], also when it fails.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
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
    let connection = connection(webview.label(), &sessions, &connections, id)?;
    log::debug!(conn = connection.ssh().info().id; "transfer {transfer_id}: uploading {local_paths:?} to {remote_dir}");
    let profile = connection.ssh().profile();
    let encoding = crate::encoding::for_profile(&profile.encoding);
    let job = transfer::Job::upload(local_paths, remote_dir, transfer::Server::of(&profile.ssh.remote), encoding);
    Ok(run_transfer(webview.label(), &sessions, &connections, &transfers, id, &transfer_id, job, None, on_progress).await)
}

/// Downloads into `local_dir` (default: the download folder from the settings). Ends with the
/// top-level items it created, and keeps all it created for [`transfer_delete`], also when it
/// fails.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
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
    let connection = connection(webview.label(), &sessions, &connections, id)?;
    let into = local_dir.as_ref().map_or_else(|| "the download folder".to_owned(), |dir| dir.display().to_string());
    log::debug!(conn = connection.ssh().info().id; "transfer {transfer_id}: downloading {remote_paths:?} to {into}");
    let local_dir = match local_dir {
        Some(dir) => dir,
        // Made if it isn't there: a disk that may be slow to answer.
        None => blocking(app, |app| app.state::<SettingsStore>().download_dir(app)).await?,
    };
    let job = transfer::Job::download(remote_paths, local_dir);
    Ok(run_transfer(webview.label(), &sessions, &connections, &transfers, id, &transfer_id, job, None, on_progress).await)
}

/// Downloads one item to `local_path`, replacing what is there (the save dialog asked). Ends
/// like [`sftp_download`].
#[tauri::command]
#[allow(clippy::too_many_arguments)]
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
    let connection = connection(webview.label(), &sessions, &connections, id)?;
    log::debug!(conn = connection.ssh().info().id; "transfer {transfer_id}: downloading {remote_path} to {}", local_path.display());
    let job = transfer::Job::download_as(remote_path, local_path);
    Ok(run_transfer(webview.label(), &sessions, &connections, &transfers, id, &transfer_id, job, None, on_progress).await)
}

/// Goes on with a failed upload or download through session `id`'s connection (the tab's
/// current one): the files copied whole are skipped (with `check`, only those still there as
/// they were left), a large file cut short goes on from where it got (see `transfer::resume`).
/// Ends like the transfer did; fails with `transfer.notResumable` if it can't be gone on with
/// (it is going on already, or was forgotten).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn transfer_resume(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    check: bool,
    on_progress: Channel<transfer::Progress>,
) -> Result<transfer::Ended> {
    let job = transfers.take_paused(webview.label(), &transfer_id).ok_or_else(|| Error::new("transfer.notResumable"))?;
    log::debug!("transfer {transfer_id}: going on with it");
    Ok(run_transfer(webview.label(), &sessions, &connections, &transfers, id, &transfer_id, job, Some(check), on_progress).await)
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
    let cleaning = crate::cleanup::hold();
    let cancelled = transfers.start(webview.label(), &edit_id);
    let result = async {
        let sftp = connection(webview.label(), &sessions, &connections, id)?.sftp().await?;
        let local = edits.local_path(&edit_id, &remote_path)?;
        let remote_stamp = edit::remote_stamp(&sftp, &remote_path).await;
        let mut reporter = transfer::Reporter::new(on_progress, cancelled.clone());
        transfer::download_to_edit(&sftp, &remote_path, &local, &mut reporter).await?;
        edits.start(webview.label(), &edit_id, remote_path, local.clone(), remote_stamp, on_event);
        Result::Ok(local)
    };
    // Watched while the download still counts as the page's: if the page went since, the
    // download was cancelled with it, and the edit, watched too late for the page to stop it,
    // stops here.
    let result = result.await.and_then(|local| {
        if !cancelled.is_cancelled() {
            return Ok(local);
        }
        edits.stop(webview.label(), &edit_id);
        Err(Error::new("transfer.cancelled"))
    });
    transfers.finish(webview.label(), &edit_id);
    drop(cleaning);
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

/// Cancels a running transfer. One that has failed meanwhile, and was kept to go on with, is
/// discarded instead (see [`discard`]), through session `id`'s connection.
#[tauri::command]
pub async fn transfer_cancel(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: Option<SessionId>,
    transfer_id: String,
) -> Result<()> {
    let jobs = transfers.cancel(webview.label(), &transfer_id);
    discard(webview.label(), &sessions, &connections, id, jobs.into_iter().collect()).await;
    Ok(())
}

/// Permanently deletes what an ended transfer created (see `transfer::left`): on the server
/// through session `id`'s connection for an upload, if it still goes to the same server. Ends
/// with what it deleted, and what is still to delete after a failure, which can be deleted
/// again.
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
    // Forgotten, or being deleted already.
    let Some(kept) = transfers.take(label, &transfer_id) else { return Ok(transfer::Deletion::default()) };
    let (kept, deletion) = match kept {
        transfer::Kept::Local(mut left) => {
            let (left, deletion) = blocking(app, move |_| {
                let deletion = transfer::delete_local(&mut left);
                Ok((left, deletion))
            })
            .await?;
            (transfer::Kept::Local(left), deletion)
        }
        transfer::Kept::Remote(mut left, server) => {
            let sftp = async {
                let connection = connection(label, &sessions, &connections, id.ok_or_else(|| Error::new("session.notConnected"))?)?;
                server.check_same(&transfer::Server::of(&connection.ssh().profile().ssh.remote), "transfer.deleteElsewhere")?;
                Ok(connection.sftp().await?)
            };
            let deletion = match sftp.await {
                Ok(sftp) => transfer::delete_remote(&sftp, &mut left).await,
                Err(e) => transfer::Deletion { error: Some(e), ..Default::default() }.with_left(&left),
            };
            (transfer::Kept::Remote(left, server), deletion)
        }
    };
    transfers.put_back(label, &transfer_id, kept);
    Ok(deletion)
}

/// Forgets what the transfers `transfer_ids` left, which the file panel no longer lists, and
/// discards those that failed (see [`discard`]).
#[tauri::command]
pub async fn transfer_forget(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: Option<SessionId>,
    transfer_ids: Vec<String>,
) -> Result<()> {
    let jobs = transfers.forget(webview.label(), &transfer_ids);
    discard(webview.label(), &sessions, &connections, id, jobs).await;
    Ok(())
}

/// Gives up on going on with a failed transfer (see [`discard`]); what it created is kept for
/// [`transfer_delete`].
#[tauri::command]
pub async fn transfer_discard(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: Option<SessionId>,
    transfer_id: String,
) -> Result<()> {
    let jobs = transfers.take_paused(webview.label(), &transfer_id);
    discard(webview.label(), &sessions, &connections, id, jobs.into_iter().collect()).await;
    Ok(())
}

/// Discards failed transfers that won't be gone on with: removes what their files cut short
/// left, an upload's through session `id`'s connection while it goes to the same server (left
/// there otherwise, a hidden file in the user's own folder), and gives back the names claimed
/// for a download's items that never arrived.
async fn discard(webview: &str, sessions: &SessionManager, connections: &Connections, id: Option<SessionId>, jobs: Vec<transfer::Job>) {
    let connection = id.and_then(|id| connection(webview, sessions, connections, id).ok());
    for job in jobs {
        let sftp = match (job.server(), &connection) {
            (Some(server), Some(connection)) if *server == transfer::Server::of(&connection.ssh().profile().ssh.remote) => {
                connection.sftp().await.ok()
            }
            _ => None,
        };
        job.discard(sftp.as_deref()).await;
    }
}
