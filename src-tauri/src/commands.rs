// Commands take injected state plus IPC arguments, so long parameter lists are expected.
#![allow(clippy::too_many_arguments)]

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::backup;
use crate::config::{AuthMethod, Folder, Item, Profile, ProfileStore};
use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::i18n;
use crate::import;
use crate::pty;
use crate::quick::{QuickCommandStore, QuickCommands};
use crate::secrets;
use crate::settings::{Settings, SettingsStore};
use crate::session::{SessionEvent, SessionId, SessionInput, SessionManager};
use crate::sftp::{self, transfer, Listing};
use crate::ssh::{self, Connections};
use crate::zmodem;

/// Selects the language for backend text; returns the locale actually used.
#[tauri::command]
pub fn set_locale(locale: String) -> &'static str {
    i18n::set_locale(&locale)
}

#[tauri::command]
pub fn settings_get(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// Stores the settings; returns them as validated (e.g. with the font size clamped).
#[tauri::command]
pub fn settings_set(store: State<'_, SettingsStore>, settings: Settings) -> Result<Settings> {
    store.set(settings)
}

#[tauri::command]
pub fn quick_commands_get(store: State<'_, QuickCommandStore>) -> QuickCommands {
    store.get()
}

/// Stores the quick commands; returns them as stored (new ones get ids).
#[tauri::command]
pub fn quick_commands_set(store: State<'_, QuickCommandStore>, commands: QuickCommands) -> Result<QuickCommands> {
    store.set(commands)
}

#[tauri::command]
pub fn profiles_list(store: State<'_, ProfileStore>) -> Vec<Profile> {
    store.list()
}

/// `password`: `None` keeps the stored password, `Some("")` clears it.
#[tauri::command]
pub fn profile_save(store: State<'_, ProfileStore>, profile: Profile, password: Option<String>) -> Result<Profile> {
    let profile = store.save(profile)?;
    match password.as_deref() {
        None => {}
        Some("") => secrets::delete_password(&profile.id)?,
        Some(password) => secrets::set_password(&profile.id, password)?,
    }
    Ok(profile)
}

#[tauri::command]
pub fn profile_set_forwards(store: State<'_, ProfileStore>, profile_id: String, forwards: Vec<ForwardRule>) -> Result<Profile> {
    store.set_forwards(&profile_id, forwards)
}

/// The default OpenSSH client config path (`~/.ssh/config`), whether or not it exists.
#[tauri::command]
pub fn ssh_config_default_path() -> Option<PathBuf> {
    import::default_path()
}

#[tauri::command]
pub fn ssh_config_scan(store: State<'_, ProfileStore>, path: PathBuf) -> Result<Vec<import::Candidate>> {
    import::scan(&path, &store.list())
}

/// Imports the selected hosts (and the jump hosts they need); returns the new profiles.
#[tauri::command]
pub fn ssh_config_import(store: State<'_, ProfileStore>, path: PathBuf, aliases: Vec<String>) -> Result<Vec<Profile>> {
    let profiles = import::plan(&path, &aliases, &store.list())?;
    store.add_all(Vec::new(), profiles.clone())?;
    Ok(profiles)
}

#[tauri::command]
pub fn folders_list(store: State<'_, ProfileStore>) -> Vec<Folder> {
    store.folders()
}

/// Creates a folder (empty id) or renames one; returns it as saved.
#[tauri::command]
pub fn folder_save(store: State<'_, ProfileStore>, folder: Folder) -> Result<Folder> {
    store.save_folder(folder)
}

/// Deletes a folder; its sessions and subfolders move up into its parent.
#[tauri::command]
pub fn folder_delete(store: State<'_, ProfileStore>, id: String) -> Result<()> {
    store.delete_folder(&id)
}

/// Moves a session or folder into `parent` (`None`: top level), before `before` (a session or
/// folder of the same kind) or at the end.
#[tauri::command]
pub fn tree_move(store: State<'_, ProfileStore>, item: Item, parent: Option<String>, before: Option<String>) -> Result<()> {
    store.move_item(item, parent, before)
}

/// Copies a session, with its saved password, as `name` right after it.
#[tauri::command]
pub fn profile_duplicate(store: State<'_, ProfileStore>, id: String, name: String) -> Result<Profile> {
    let copy = store.duplicate(&id, &name)?;
    if let Some(password) = secrets::get_password(&id) {
        secrets::set_password(&copy.id, &password)?;
    }
    Ok(copy)
}

/// Writes all sessions and folders (without passwords) to `path`.
#[tauri::command]
pub fn sessions_export(store: State<'_, ProfileStore>, path: PathBuf) -> Result<()> {
    backup::export(&path, store.folders(), store.list())
}

#[tauri::command]
pub fn sessions_import_scan(store: State<'_, ProfileStore>, path: PathBuf) -> Result<Vec<backup::Candidate>> {
    backup::scan(&path, &store.list())
}

/// Imports the sessions `ids` (ids in the file) and what they need (jump hosts, folders).
#[tauri::command]
pub fn sessions_import(store: State<'_, ProfileStore>, path: PathBuf, ids: Vec<String>) -> Result<()> {
    let (folders, profiles) = backup::plan(&path, &ids, &store.list(), &store.folders())?;
    store.add_all(folders, profiles)
}

#[tauri::command]
pub fn profile_delete(store: State<'_, ProfileStore>, id: String) -> Result<()> {
    store.delete(&id)?;
    secrets::delete_password(&id)?;
    Ok(())
}

#[tauri::command]
pub fn ssh_open(
    store: State<'_, ProfileStore>,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    profile_id: String,
    cols: u16,
    rows: u16,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let profile = store.get(&profile_id)?;
    let jumps = store.jump_hosts(&profile)?;
    let connections = connections.inner().clone();
    Ok(sessions.spawn(on_output, on_event, (cols, rows), |id, io| ssh::run(profile, jumps, id, io, connections)))
}

/// Connects to `username@host:port` without a saved session (quick connect), with automatic
/// authentication. An empty `username` means the local user name, as with `ssh host`.
#[tauri::command]
pub fn ssh_quick_open(
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    username: String,
    host: String,
    port: u16,
    cols: u16,
    rows: u16,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let username = match username.trim() {
        "" => local_username(),
        name => name.to_owned(),
    };
    let profile = Profile {
        id: String::new(),
        name: format!("{username}@{host}"),
        host: host.trim().to_owned(),
        port,
        username,
        auth: AuthMethod::Auto,
        jump_hosts: Vec::new(),
        keepalive_interval: 30,
        auto_reconnect: true,
        forwards: Vec::new(),
        folder: None,
        command_group: None,
    };
    if profile.host.is_empty() || profile.username.is_empty() || port == 0 {
        return Err(Error::new("profile.missingFields"));
    }
    let connections = connections.inner().clone();
    Ok(sessions.spawn(on_output, on_event, (cols, rows), |id, io| ssh::run(profile, Vec::new(), id, io, connections)))
}

/// The user name `ssh` uses when none is given.
#[tauri::command]
pub fn local_username() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
}

/// Opens another shell on the SSH connection of session `source` (duplicating its tab),
/// without connecting or authenticating again. Fails with `session.notConnected` if that
/// session has no connection (any more).
#[tauri::command]
pub fn ssh_open_shared(
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    source: SessionId,
    cols: u16,
    rows: u16,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let connection = connections.get(source)?;
    let connections = connections.inner().clone();
    Ok(sessions.spawn(on_output, on_event, (cols, rows), |id, io| {
        connections.attach(id, connection.clone(), io.sink());
        ssh::run_shared(connection, id, io, connections)
    }))
}

/// Starts the user's default shell in a local pseudo terminal.
#[tauri::command]
pub fn local_open(
    sessions: State<'_, SessionManager>,
    cols: u16,
    rows: u16,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> SessionId {
    let shell = pty::default_shell();
    sessions.spawn(on_output, on_event, (cols, rows), |_, io| pty::run(shell, io))
}

/// Short name of the default local shell, e.g. "zsh" or "pwsh".
#[tauri::command]
pub fn local_shell_name() -> String {
    pty::default_shell().name()
}

#[tauri::command]
pub fn session_write(sessions: State<'_, SessionManager>, id: SessionId, data: String) -> Result<()> {
    sessions.send(id, SessionInput::Data(data.into_bytes()))
}

#[tauri::command]
pub fn session_resize(sessions: State<'_, SessionManager>, id: SessionId, cols: u16, rows: u16) -> Result<()> {
    sessions.send(id, SessionInput::Resize { cols, rows })
}

/// Records that the frontend has processed `bytes` of the session's output.
#[tauri::command]
pub fn session_ack(sessions: State<'_, SessionManager>, id: SessionId, bytes: usize) -> Result<()> {
    sessions.ack(id, bytes)
}

/// The program running in the session's terminal other than the shell (an empty string if
/// its name is unknown), or `None`. Only local terminals can tell.
#[tauri::command]
pub fn session_foreground(sessions: State<'_, SessionManager>, id: SessionId) -> Result<Option<String>> {
    sessions.foreground(id)
}

#[tauri::command]
pub fn session_close(sessions: State<'_, SessionManager>, connections: State<'_, Connections>, id: SessionId) -> Result<()> {
    connections.close(id);
    sessions.remove(id)
}

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
    let sftp = connections.get(id)?.sftp().await?;
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&transfer_id));
    let result = transfer::upload(&sftp, &local_paths, &remote_dir, &mut reporter).await;
    transfers.finish(&transfer_id);
    Ok(result?)
}

/// Downloads into `local_dir` (default: the Downloads folder); returns the created paths.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    connections: State<'_, Connections>,
    transfers: State<'_, transfer::Transfers>,
    id: SessionId,
    transfer_id: String,
    remote_paths: Vec<String>,
    local_dir: Option<PathBuf>,
    on_progress: Channel<transfer::Progress>,
) -> Result<Vec<PathBuf>> {
    let sftp = connections.get(id)?.sftp().await?;
    let local_dir = match local_dir {
        Some(dir) => dir,
        None => app.path().download_dir()?,
    };
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&transfer_id));
    let result = transfer::download(&sftp, &remote_paths, &local_dir, &mut reporter).await;
    transfers.finish(&transfer_id);
    Ok(result?)
}

#[tauri::command]
pub fn transfer_cancel(transfers: State<'_, transfer::Transfers>, transfer_id: String) {
    transfers.cancel(&transfer_id);
}

/// Starts (or restarts with a new definition) a forwarding rule on the session's connection.
/// Progress is reported through the session's event channel.
#[tauri::command]
pub fn forward_start(connections: State<'_, Connections>, id: SessionId, rule: ForwardRule) -> Result<()> {
    let rule = rule.normalize()?;
    connections.get(id)?.forwards().start(rule, false);
    Ok(())
}

#[tauri::command]
pub fn forward_stop(connections: State<'_, Connections>, id: SessionId, rule_id: String) -> Result<()> {
    connections.get(id)?.forwards().stop(&rule_id);
    Ok(())
}

/// Answers a ZMODEM download (`sz`): save into `dir`, or the Downloads folder.
#[tauri::command]
pub fn zmodem_save_to(app: AppHandle, sessions: State<'_, SessionManager>, id: SessionId, dir: Option<PathBuf>) -> Result<()> {
    let dir = match dir {
        Some(dir) => dir,
        None => app.path().download_dir()?,
    };
    sessions.zmodem(id)?.reply(zmodem::Reply::Destination(dir));
    Ok(())
}

/// Answers a ZMODEM upload (`rz`) with the files to send.
#[tauri::command]
pub fn zmodem_send_files(sessions: State<'_, SessionManager>, id: SessionId, paths: Vec<PathBuf>) -> Result<()> {
    sessions.zmodem(id)?.reply(zmodem::Reply::Files(paths));
    Ok(())
}

/// Cancels the session's ZMODEM transfer, including one waiting for an answer.
#[tauri::command]
pub fn zmodem_cancel(sessions: State<'_, SessionManager>, id: SessionId) -> Result<()> {
    sessions.zmodem(id)?.cancel();
    Ok(())
}
