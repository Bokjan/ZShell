// Commands take injected state plus IPC arguments, so long parameter lists are expected.
#![allow(clippy::too_many_arguments)]

use std::path::PathBuf;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::backup;
use crate::config::{Folder, Item, Profile, ProfileStore, Protocol};
use crate::encoding;
use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::i18n;
use crate::import;
use crate::logging::{LogInfo, LogOpen, LogSlot, LogSummary, Logs};
use crate::net::Route;
use crate::proxy::Proxy;
use crate::pty;
use crate::quick::{QuickCommandStore, QuickCommands};
use crate::secrets;
use crate::serial;
use crate::settings::{Settings, SettingsStore};
use crate::session::{SessionEvent, SessionId, SessionInput, SessionManager};
use crate::sftp::drag;
use crate::sftp::edit::{self, EditEvent, Edits};
use crate::sftp::{self, transfer, Listing};
use crate::ssh::{self, known_hosts, Connections};
use crate::telnet;
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
pub fn settings_set(store: State<'_, SettingsStore>, logs: State<'_, Logs>, settings: Settings) -> Result<Settings> {
    let settings = store.set(settings)?;
    // A shorter retention applies right away.
    logs.clean_up(settings.logs.keep_days);
    Ok(settings)
}

/// The folder the sessions and settings are saved in; created if needed, to be shown in the
/// file manager.
#[tauri::command]
pub fn config_directory(app: AppHandle) -> Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir)
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

#[tauri::command]
pub fn proxies_list(store: State<'_, ProfileStore>) -> Vec<Proxy> {
    store.proxies()
}

/// `password`: `None` keeps the stored password, `Some("")` clears it. A proxy without a user
/// name keeps none.
#[tauri::command]
pub fn proxy_save(store: State<'_, ProfileStore>, proxy: Proxy, password: Option<String>) -> Result<Proxy> {
    let had_password = store.proxy(&proxy.id).is_ok_and(|previous| previous.uses_password());
    let proxy = store.save_proxy(proxy)?;
    match password.as_deref() {
        // The keychain is only touched when there can be a password to remove.
        _ if !proxy.uses_password() => {
            if had_password {
                secrets::delete_proxy_password(&proxy.id)?;
            }
        }
        None => {}
        Some("") => secrets::delete_proxy_password(&proxy.id)?,
        Some(password) => secrets::set_proxy_password(&proxy.id, password)?,
    }
    Ok(proxy)
}

/// Deletes a proxy no session uses (`proxy.inUse` otherwise), with its password.
#[tauri::command]
pub fn proxy_delete(store: State<'_, ProfileStore>, id: String) -> Result<()> {
    store.delete_proxy(&id)?;
    secrets::delete_proxy_password(&id)?;
    Ok(())
}

/// The default OpenSSH client config path (`~/.ssh/config`), whether or not it exists.
#[tauri::command]
pub fn ssh_config_default_path() -> Option<PathBuf> {
    import::default_path()
}

/// `~/.ssh/known_hosts`, whether or not it exists.
#[tauri::command]
pub fn known_hosts_path() -> Option<PathBuf> {
    known_hosts::path()
}

/// The host keys in known_hosts; empty if there is no file.
#[tauri::command]
pub fn known_hosts_list() -> Result<Vec<known_hosts::Entry>> {
    known_hosts::list()
}

/// The lines of the entries for a host (`host`, `host:port` or `[host]:port`), including
/// hashed ones.
#[tauri::command]
pub fn known_hosts_find(query: String) -> Result<Vec<usize>> {
    known_hosts::find(&query)
}

/// Removes the entry on `line`, if the line still reads `text`.
#[tauri::command]
pub fn known_hosts_remove(line: usize, text: String) -> Result<()> {
    known_hosts::remove(line, &text)
}

#[tauri::command]
pub fn ssh_config_scan(store: State<'_, ProfileStore>, path: PathBuf) -> Result<Vec<import::Candidate>> {
    import::scan(&path, &store.list())
}

/// Imports the selected hosts (and the jump hosts and proxy commands they need); returns the
/// new profiles.
#[tauri::command]
pub fn ssh_config_import(store: State<'_, ProfileStore>, path: PathBuf, aliases: Vec<String>) -> Result<Vec<Profile>> {
    let (proxies, profiles) = import::plan(&path, &aliases, &store.list(), &store.proxies())?;
    store.add_all(Vec::new(), proxies, profiles.clone())?;
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

/// Writes all sessions, folders and proxies (without passwords) to `path`.
#[tauri::command]
pub fn sessions_export(store: State<'_, ProfileStore>, path: PathBuf) -> Result<()> {
    backup::export(&path, store.folders(), store.proxies(), store.list())
}

#[tauri::command]
pub fn sessions_import_scan(store: State<'_, ProfileStore>, path: PathBuf) -> Result<Vec<backup::Candidate>> {
    backup::scan(&path, &store.list())
}

/// Imports the sessions `ids` (ids in the file) and what they need (jump hosts, proxies,
/// folders).
#[tauri::command]
pub fn sessions_import(store: State<'_, ProfileStore>, path: PathBuf, ids: Vec<String>) -> Result<()> {
    let (profiles, folders, proxies) = (store.list(), store.folders(), store.proxies());
    let here = backup::Here { profiles: &profiles, folders: &folders, proxies: &proxies };
    let plan = backup::plan(&path, &ids, &here)?;
    store.add_all(plan.folders, plan.proxies, plan.profiles)
}

#[tauri::command]
pub fn profile_delete(store: State<'_, ProfileStore>, id: String) -> Result<()> {
    store.delete(&id)?;
    secrets::delete_password(&id)?;
    Ok(())
}

/// Opens a session from a saved profile, with the backend for its protocol.
#[tauri::command]
pub fn profile_open(
    store: State<'_, ProfileStore>,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    logs: State<'_, Logs>,
    settings: State<'_, SettingsStore>,
    profile_id: String,
    cols: u16,
    rows: u16,
    log: Option<LogOpen>,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let profile = store.get(&profile_id)?;
    let route = match profile.protocol {
        Protocol::Serial => Route::default(),
        _ => store.route(&profile)?,
    };
    let slot = LogSlot::new(LogInfo { profile: Some(profile.id.clone()), ..log_info(&profile) });
    let opened = logs.open(&slot, log.unwrap_or_default(), profile.auto_log, &settings.get().logs);
    let encoding = encoding::for_profile(&profile.encoding);
    let size = (cols, rows);
    let id = match profile.protocol {
        Protocol::Ssh => {
            let connections = connections.inner().clone();
            sessions.spawn(on_output, on_event, size, slot, encoding, |id, io| ssh::run(profile, route, id, io, connections))
        }
        Protocol::Telnet => sessions.spawn(on_output, on_event, size, slot, encoding, |_, io| {
            let profile_id = profile.id.clone();
            telnet::run(profile, route, move || secrets::get_password(&profile_id), io)
        }),
        Protocol::Serial => sessions.spawn(on_output, on_event, size, slot, encoding, |_, io| serial::run(profile.serial, io)),
    };
    report_log(&sessions, id, opened);
    Ok(id)
}

/// How a session's log names it: by the profile, its address and user.
fn log_info(profile: &Profile) -> LogInfo {
    let host = match profile.protocol {
        Protocol::Serial => profile.serial.device.clone(),
        _ => profile.host.clone(),
    };
    LogInfo { session: profile.name.clone(), host, user: profile.username.clone(), profile: None }
}

/// Tells a new session's tab about the log it started with, or why that failed.
fn report_log(sessions: &SessionManager, id: SessionId, opened: Option<Result<PathBuf>>) {
    if let (Some(opened), Ok(sink)) = (opened, sessions.sink(id)) {
        sink.event(log_event(opened));
    }
}

fn log_event(result: Result<PathBuf>) -> SessionEvent {
    match result {
        Ok(path) => SessionEvent::Log { path: Some(path.display().to_string()), error: None },
        Err(error) => SessionEvent::Log { path: None, error: Some(error) },
    }
}

/// Opens an SSH or Telnet session to an address typed into the search box, without a saved
/// profile. SSH without a user name uses the local one, like `ssh host`.
#[tauri::command]
pub fn quick_open(
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    logs: State<'_, Logs>,
    settings: State<'_, SettingsStore>,
    protocol: Protocol,
    username: String,
    host: String,
    port: u16,
    cols: u16,
    rows: u16,
    log: Option<LogOpen>,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let username = match username.trim() {
        "" if protocol == Protocol::Ssh => local_username(),
        name => name.to_owned(),
    };
    let host = host.trim().to_owned();
    let name = if username.is_empty() { host.clone() } else { format!("{username}@{host}") };
    let profile = Profile { protocol, ..Profile::new(name, host, port, username) };
    match protocol {
        Protocol::Ssh if profile.username.is_empty() => return Err(Error::new("profile.missingFields")),
        Protocol::Serial => return Err(Error::new("profile.missingDevice")),
        _ if profile.host.is_empty() || port == 0 => return Err(Error::new("profile.missingHost")),
        _ => {}
    }
    // Logged only when started by hand: there is no session to record automatically.
    let slot = LogSlot::new(log_info(&profile));
    let opened = logs.open(&slot, log.unwrap_or_default(), false, &settings.get().logs);
    let size = (cols, rows);
    let utf8 = encoding_rs::UTF_8;
    let id = match protocol {
        Protocol::Telnet => sessions.spawn(on_output, on_event, size, slot, utf8, |_, io| telnet::run(profile, Route::default(), || None, io)),
        _ => {
            let connections = connections.inner().clone();
            sessions.spawn(on_output, on_event, size, slot, utf8, |id, io| ssh::run(profile, Route::default(), id, io, connections))
        }
    };
    report_log(&sessions, id, opened);
    Ok(id)
}

/// The user's home folder, which `~` stands for in key paths (and in `~/.ssh`).
#[tauri::command]
pub fn home_directory() -> Option<PathBuf> {
    std::env::home_dir()
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
    store: State<'_, ProfileStore>,
    sessions: State<'_, SessionManager>,
    connections: State<'_, Connections>,
    logs: State<'_, Logs>,
    settings: State<'_, SettingsStore>,
    source: SessionId,
    cols: u16,
    rows: u16,
    log: Option<LogOpen>,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let connection = connections.get(source)?;
    let connections = connections.inner().clone();
    // Named like the source's log; a log of its own, as a new tab of the session would get.
    let info = sessions.sink(source)?.log().info.clone();
    let auto = info.profile.as_ref().and_then(|id| store.get(id).ok()).is_some_and(|p| p.auto_log);
    let slot = LogSlot::new(info);
    let opened = logs.open(&slot, log.unwrap_or_default(), auto, &settings.get().logs);
    let encoding = encoding::for_profile(&connection.profile().encoding);
    let id = sessions.spawn(on_output, on_event, (cols, rows), slot, encoding, |id, io| {
        connections.attach(id, connection.clone(), io.sink());
        ssh::run_shared(connection, id, io, connections)
    });
    report_log(&sessions, id, opened);
    Ok(id)
}

/// Starts the user's default shell in a local pseudo terminal.
#[tauri::command]
pub fn local_open(
    sessions: State<'_, SessionManager>,
    logs: State<'_, Logs>,
    settings: State<'_, SettingsStore>,
    cols: u16,
    rows: u16,
    log: Option<LogOpen>,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> SessionId {
    let shell = pty::default_shell();
    let settings = settings.get().logs;
    let slot = LogSlot::new(LogInfo { session: shell.name(), host: "localhost".to_owned(), user: local_username(), profile: None });
    let opened = logs.open(&slot, log.unwrap_or_default(), settings.auto_local, &settings);
    let id = sessions.spawn(on_output, on_event, (cols, rows), slot, encoding_rs::UTF_8, |_, io| pty::run(shell, io));
    report_log(&sessions, id, opened);
    id
}

/// Starts logging a session by hand, in a new file; returns its path.
#[tauri::command]
pub fn session_log_start(
    sessions: State<'_, SessionManager>,
    logs: State<'_, Logs>,
    settings: State<'_, SettingsStore>,
    id: SessionId,
) -> Result<PathBuf> {
    let sink = sessions.sink(id)?;
    let path = logs.start(sink.log(), &settings.get().logs)?;
    sink.event(log_event(Ok(path.clone())));
    Ok(path)
}

#[tauri::command]
pub fn session_log_stop(sessions: State<'_, SessionManager>, logs: State<'_, Logs>, id: SessionId) -> Result<()> {
    let sink = sessions.sink(id)?;
    logs.stop(sink.log());
    sink.event(SessionEvent::Log { path: None, error: None });
    Ok(())
}

/// How many logs ZShell has written (and still exist), and their size.
#[tauri::command]
pub fn logs_summary(logs: State<'_, Logs>) -> LogSummary {
    logs.summary()
}

/// Where new logs go; created if needed, to be shown in the file manager.
#[tauri::command]
pub fn logs_directory(logs: State<'_, Logs>, settings: State<'_, SettingsStore>) -> PathBuf {
    let dir = logs.directory(&settings.get().logs);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// How many logs a saved session has.
#[tauri::command]
pub fn logs_count(logs: State<'_, Logs>, profile_id: String) -> usize {
    logs.count(&profile_id)
}

/// Deletes the logs of a saved session, or all logs (`profile_id` absent), except those
/// being written; returns how many were deleted.
#[tauri::command]
pub fn logs_delete(logs: State<'_, Logs>, profile_id: Option<String>) -> usize {
    match profile_id {
        Some(id) => logs.delete_for(&id),
        None => logs.delete_all(),
    }
}

/// Short name of the default local shell, e.g. "zsh" or "pwsh"; none when the system doesn't
/// allow local terminals (Windows in S mode).
#[tauri::command]
pub fn local_shell_name() -> Option<String> {
    pty::local_shells_allowed().then(|| pty::default_shell().name())
}

#[tauri::command]
pub fn session_write(sessions: State<'_, SessionManager>, id: SessionId, data: String) -> Result<()> {
    sessions.send(id, SessionInput::Data(data.into_bytes()))
}

/// Sends a break (serial and Telnet sessions).
#[tauri::command]
pub fn session_break(sessions: State<'_, SessionManager>, id: SessionId) -> Result<()> {
    sessions.send(id, SessionInput::Break)
}

/// The serial ports on this computer.
#[tauri::command]
pub fn serial_ports() -> Vec<serial::PortInfo> {
    serial::ports()
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
    let sftp = connections.get(id)?.sftp().await?;
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
    let sftp = connections.get(id)?.sftp().await?;
    let mut reporter = transfer::Reporter::new(on_progress, transfers.start(&transfer_id));
    let result = transfer::download_to(&sftp, &[(remote_path, local_path.clone())], &mut reporter).await;
    transfers.finish(&transfer_id);
    result?;
    Ok(local_path)
}

/// Where downloads go without asking: the folder chosen in the settings, else Downloads.
fn download_dir(app: &AppHandle, settings: &SettingsStore) -> Result<PathBuf> {
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
pub fn downloads_directory(app: AppHandle, settings: State<'_, SettingsStore>) -> Result<PathBuf> {
    download_dir(&app, &settings)
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
    let sftp = connections.get(id)?.sftp().await?;
    drag::drag_out(&window, sftp, transfer_id, items, on_event).await
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

/// Answers a ZMODEM download (`sz`): save into `dir`, or the download folder.
#[tauri::command]
pub fn zmodem_save_to(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    settings: State<'_, SettingsStore>,
    id: SessionId,
    dir: Option<PathBuf>,
) -> Result<()> {
    let dir = match dir {
        Some(dir) => dir,
        None => download_dir(&app, &settings)?,
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
