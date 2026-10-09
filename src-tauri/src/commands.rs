// Commands take injected state plus IPC arguments, so long parameter lists are expected.
#![allow(clippy::too_many_arguments)]

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview, WebviewWindow};
use ts_rs::TS;

use crate::backup;
use crate::config::{Additions, Connection, Folder, Item, Profile, ProfileStore, Protocol, Remote, SerialOptions, SetAside, SetAsideFile, SshOptions, SshProfile};
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
use crate::session::{SessionEvent, SessionId, SessionInput, SessionManager, TermIo};
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

/// Runs a command's blocking work (files, the keychain, the system's device list) on the
/// blocking pool. Sync commands run on the main thread, where waiting freezes the window: on a
/// slow or network disk, or while macOS asks about each keychain entry after an update.
///
/// Async commands may run in any order, though. Those replacing a whole value that the
/// frontend sends again before the last one is done (the settings, the quick commands, a
/// session's forwarding rules) stay sync, so that the last one sent is the one kept.
async fn blocking<T: Send + 'static>(app: AppHandle, work: impl FnOnce(&AppHandle) -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app)).await?
}

/// Starts a new session's log (see [`Logs::open`]) on the blocking pool.
async fn open_log(app: &AppHandle, slot: &Arc<LogSlot>, how: Option<LogOpen>, auto: bool) -> Result<Option<Result<PathBuf>>> {
    let slot = slot.clone();
    blocking(app.clone(), move |app| {
        let settings = app.state::<SettingsStore>().get().logs;
        Ok(app.state::<Logs>().open(&slot, how.unwrap_or_default(), auto, &settings))
    })
    .await
}

#[tauri::command]
pub fn settings_get(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// Stores the settings; returns them as validated (e.g. with the font size clamped).
#[tauri::command]
pub fn settings_set(app: AppHandle, store: State<'_, SettingsStore>, settings: Settings) -> Result<Settings> {
    let settings = store.set(settings)?;
    // A shorter retention applies right away; checking every log can take a while.
    let keep_days = settings.logs.keep_days;
    tauri::async_runtime::spawn_blocking(move || app.state::<Logs>().clean_up(keep_days));
    Ok(settings)
}

/// The folder the sessions and settings are saved in; created if needed, to be shown in the
/// file manager.
#[tauri::command]
pub async fn config_directory(app: AppHandle) -> Result<PathBuf> {
    blocking(app, |app| {
        let dir = app.path().app_config_dir()?;
        let _ = std::fs::create_dir_all(&dir);
        Ok(dir)
    })
    .await
}

/// The data files that could not be read at startup and were set aside; reported once.
#[tauri::command]
pub fn config_set_aside(set_aside: State<'_, SetAside>) -> Vec<SetAsideFile> {
    set_aside.take()
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

/// A saved session or proxy, with the error storing its password gave, if any. It is saved
/// either way: the dialog then goes on editing it, so that saving again doesn't add another.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Saved<T> {
    saved: T,
    password_error: Option<Error>,
}

/// `password`: `None` keeps the stored password, `Some("")` clears it.
#[tauri::command]
pub async fn profile_save(app: AppHandle, profile: Profile, password: Option<String>) -> Result<Saved<Profile>> {
    blocking(app, move |app| {
        let profile = app.state::<ProfileStore>().save(profile)?;
        // A session that is no longer SSH has no forwarding rules; running ones stop.
        let rules: Vec<String> = profile.forwards().iter().map(|rule| rule.id.clone()).collect();
        app.state::<Connections>().retain_forwards(&profile.id, &rules);
        let stored = match password.as_deref() {
            None => Ok(()),
            Some("") => secrets::delete_password(&profile.id),
            Some(password) => secrets::set_password(&profile.id, password),
        };
        Ok(Saved { saved: profile, password_error: stored.err().map(Error::from) })
    })
    .await
}

#[tauri::command]
pub fn profile_set_forwards(
    store: State<'_, ProfileStore>,
    connections: State<'_, Connections>,
    profile_id: String,
    forwards: Vec<ForwardRule>,
) -> Result<Profile> {
    let profile = store.set_forwards(&profile_id, forwards)?;
    // A rule deleted while running (perhaps on another tab's connection) stops.
    let ids: Vec<String> = profile.forwards().iter().map(|rule| rule.id.clone()).collect();
    connections.retain_forwards(&profile_id, &ids);
    Ok(profile)
}

#[tauri::command]
pub fn proxies_list(store: State<'_, ProfileStore>) -> Vec<Proxy> {
    store.proxies()
}

/// `password`: `None` keeps the stored password, `Some("")` clears it. A proxy without a user
/// name keeps none.
#[tauri::command]
pub async fn proxy_save(app: AppHandle, proxy: Proxy, password: Option<String>) -> Result<Saved<Proxy>> {
    blocking(app, move |app| {
        let store = app.state::<ProfileStore>();
        let had_password = store.proxy(&proxy.id).is_ok_and(|previous| previous.uses_password());
        let proxy = store.save_proxy(proxy)?;
        let stored = match password.as_deref() {
            // The keychain is only touched when there can be a password to remove.
            _ if !proxy.uses_password() => match had_password {
                true => secrets::delete_proxy_password(&proxy.id),
                false => Ok(()),
            },
            None => Ok(()),
            Some("") => secrets::delete_proxy_password(&proxy.id),
            Some(password) => secrets::set_proxy_password(&proxy.id, password),
        };
        Ok(Saved { saved: proxy, password_error: stored.err().map(Error::from) })
    })
    .await
}

/// Deletes a proxy no session uses (`proxy.inUse` otherwise), with its password.
#[tauri::command]
pub async fn proxy_delete(app: AppHandle, id: String) -> Result<()> {
    blocking(app, move |app| {
        app.state::<ProfileStore>().delete_proxy(&id)?;
        secrets::delete_proxy_password(&id)?;
        Ok(())
    })
    .await
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
pub async fn known_hosts_list(app: AppHandle) -> Result<Vec<known_hosts::Entry>> {
    blocking(app, |_| known_hosts::list()).await
}

/// The lines of the entries for a host (`host`, `host:port` or `[host]:port`), including
/// hashed ones.
#[tauri::command]
pub async fn known_hosts_find(app: AppHandle, query: String) -> Result<Vec<usize>> {
    blocking(app, move |_| known_hosts::find(&query)).await
}

/// Removes the entry on `line`, if the line still reads `text`.
#[tauri::command]
pub async fn known_hosts_remove(app: AppHandle, line: usize, text: String) -> Result<()> {
    blocking(app, move |_| known_hosts::remove(line, &text)).await
}

#[tauri::command]
pub async fn ssh_config_scan(app: AppHandle, path: PathBuf) -> Result<Vec<import::Candidate>> {
    blocking(app, move |app| import::scan(&path, &app.state::<ProfileStore>().list())).await
}

/// Imports the selected hosts (and the jump hosts and proxy commands they need); returns the
/// new profiles.
#[tauri::command]
pub async fn ssh_config_import(app: AppHandle, path: PathBuf, aliases: Vec<String>) -> Result<Vec<Profile>> {
    blocking(app, move |app| {
        app.state::<ProfileStore>().add_all(|here| {
            let (proxies, profiles) = import::plan(&path, &aliases, here.profiles, here.proxies)?;
            Ok(Additions { proxies, profiles, ..Additions::default() })
        })
    })
    .await
}

#[tauri::command]
pub fn folders_list(store: State<'_, ProfileStore>) -> Vec<Folder> {
    store.folders()
}

/// Creates a folder (empty id) or renames one; returns it as saved.
#[tauri::command]
pub async fn folder_save(app: AppHandle, folder: Folder) -> Result<Folder> {
    blocking(app, move |app| app.state::<ProfileStore>().save_folder(folder)).await
}

/// Deletes a folder; its sessions and subfolders move up into its parent.
#[tauri::command]
pub async fn folder_delete(app: AppHandle, id: String) -> Result<()> {
    blocking(app, move |app| app.state::<ProfileStore>().delete_folder(&id)).await
}

/// Moves a session or folder into `parent` (`None`: top level), before `before` (a session or
/// folder of the same kind) or at the end.
#[tauri::command]
pub async fn tree_move(app: AppHandle, item: Item, parent: Option<String>, before: Option<String>) -> Result<()> {
    blocking(app, move |app| app.state::<ProfileStore>().move_item(item, parent, before)).await
}

/// Copies a session, with its saved password, as `name` right after it.
#[tauri::command]
pub async fn profile_duplicate(app: AppHandle, id: String, name: String) -> Result<Profile> {
    blocking(app, move |app| {
        let copy = app.state::<ProfileStore>().duplicate(&id, &name)?;
        if let Some(password) = secrets::get_password(&id) {
            secrets::set_password(&copy.id, &password)?;
        }
        Ok(copy)
    })
    .await
}

/// Writes all sessions, folders and proxies (without passwords) to `path`.
#[tauri::command]
pub async fn sessions_export(app: AppHandle, path: PathBuf) -> Result<()> {
    blocking(app, move |app| {
        let store = app.state::<ProfileStore>();
        backup::export(&path, store.folders(), store.proxies(), store.list())
    })
    .await
}

#[tauri::command]
pub async fn sessions_import_scan(app: AppHandle, path: PathBuf) -> Result<Vec<backup::Candidate>> {
    blocking(app, move |app| backup::scan(&path, &app.state::<ProfileStore>().list())).await
}

/// Imports the sessions `ids` (ids in the file) and what they need (jump hosts, proxies,
/// folders).
#[tauri::command]
pub async fn sessions_import(app: AppHandle, path: PathBuf, ids: Vec<String>) -> Result<()> {
    blocking(app, move |app| app.state::<ProfileStore>().add_all(|here| backup::plan(&path, &ids, here)).map(drop)).await
}

#[tauri::command]
pub async fn profile_delete(app: AppHandle, id: String) -> Result<()> {
    blocking(app, move |app| {
        app.state::<ProfileStore>().delete(&id)?;
        secrets::delete_password(&id)?;
        Ok(())
    })
    .await
}

/// How a session's log names it: by the profile, its address and user.
fn log_info(profile: &Profile) -> LogInfo {
    let (host, user) = match &profile.connection {
        Connection::Serial(serial) => (serial.device.clone(), String::new()),
        Connection::Ssh(SshOptions { remote, .. }) | Connection::Telnet(remote) => (remote.host.clone(), remote.username.clone()),
    };
    LogInfo { session: profile.name.clone(), host, user, profile: None }
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

/// What a new session runs.
#[derive(Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionSpec {
    /// A saved session, with the backend for its protocol. `carry`: forwarding rules to start
    /// besides the automatic ones (those running before the tab reconnected).
    Profile { profile_id: String, carry: Vec<String> },
    /// An SSH or Telnet connection typed into the search box, without a saved session. SSH
    /// without a user name uses the local one, like `ssh host`.
    Quick { protocol: Protocol, username: String, host: String, port: u16 },
    /// Another shell on the SSH connection of session `source` (duplicating its tab), without
    /// connecting or authenticating again. Fails with `session.notConnected` if that session
    /// has no connection (any more).
    Shared { source: SessionId },
    /// The user's default shell in a local pseudo terminal.
    Local,
}

/// Starts a session; returns its id. `channel` carries the terminal's output and the
/// session's events: each message is a type byte (0 output, 1 event as JSON) and its content.
#[tauri::command]
pub async fn session_open(
    app: AppHandle,
    webview: Webview,
    sessions: State<'_, SessionManager>,
    spec: SessionSpec,
    cols: u16,
    rows: u16,
    log: Option<LogOpen>,
    channel: Channel,
) -> Result<SessionId> {
    let Launch { log_info, auto_log, encoding, backend } = launch(&app, spec)?;
    let slot = LogSlot::new(log_info);
    let opened = open_log(&app, &slot, log, auto_log).await?;
    let id = sessions.spawn(webview.label(), channel, (cols, rows), slot, encoding, |id, io| backend.start(id, io));
    report_log(&sessions, id, opened);
    Ok(id)
}

/// A session about to start: how its log is named and whether it starts logged, the remote
/// side's character encoding, and its backend.
struct Launch {
    log_info: LogInfo,
    auto_log: bool,
    encoding: &'static encoding_rs::Encoding,
    backend: Backend,
}

fn launch(app: &AppHandle, spec: SessionSpec) -> Result<Launch> {
    let connections = app.state::<Connections>().inner().clone();
    let utf8 = encoding_rs::UTF_8;
    match spec {
        SessionSpec::Profile { profile_id, carry } => {
            let store = app.state::<ProfileStore>();
            let profile = store.get(&profile_id)?;
            let log_info = LogInfo { profile: Some(profile.id.clone()), ..log_info(&profile) };
            let (auto_log, encoding) = (profile.auto_log, encoding::for_profile(&profile.encoding));
            let Profile { id, name, encoding: label, connection, .. } = profile;
            let backend = match connection {
                Connection::Ssh(ssh) => {
                    let route = store.route(&ssh.remote)?;
                    Backend::Ssh { profile: SshProfile { id, name, encoding: label, ssh }, route, connections, carry }
                }
                Connection::Telnet(remote) => Backend::Telnet { route: store.route(&remote)?, remote, password_of: Some(id) },
                Connection::Serial(serial) => Backend::Serial(serial),
            };
            Ok(Launch { log_info, auto_log, encoding, backend })
        }
        SessionSpec::Quick { protocol, username, host, port } => {
            let username = match username.trim() {
                "" if protocol == Protocol::Ssh => local_username(),
                name => name.to_owned(),
            };
            let host = host.trim().to_owned();
            let name = if username.is_empty() { host.clone() } else { format!("{username}@{host}") };
            match protocol {
                Protocol::Ssh if username.is_empty() => return Err(Error::new("profile.missingFields")),
                Protocol::Serial => return Err(Error::new("profile.missingDevice")),
                _ if host.is_empty() || port == 0 => return Err(Error::new("profile.missingHost")),
                _ => {}
            }
            let log_info = LogInfo { session: name.clone(), host: host.clone(), user: username.clone(), profile: None };
            let remote = Remote::new(host, port, username);
            let backend = match protocol {
                Protocol::Telnet => Backend::Telnet { remote, route: Route::default(), password_of: None },
                _ => Backend::Ssh { profile: SshProfile::quick(name, remote), route: Route::default(), connections, carry: Vec::new() },
            };
            // Logged only when started by hand: there is no session to record automatically.
            Ok(Launch { log_info, auto_log: false, encoding: utf8, backend })
        }
        SessionSpec::Shared { source } => {
            let connection = connections.get(source)?;
            // Ended, but its session hasn't noticed yet (it is about to report it lost).
            if connection.handle().is_closed() {
                return Err(Error::new("session.notConnected"));
            }
            // Named like the source's log; a log of its own, as a new tab of the session would get.
            let log_info = app.state::<SessionManager>().sink(source)?.log().info.clone();
            let store = app.state::<ProfileStore>();
            let auto_log = log_info.profile.as_ref().and_then(|id| store.get(id).ok()).is_some_and(|p| p.auto_log);
            let encoding = encoding::for_profile(&connection.profile().encoding);
            Ok(Launch { log_info, auto_log, encoding, backend: Backend::Shared { connection, connections } })
        }
        SessionSpec::Local => {
            let shell = pty::default_shell();
            let log_info = LogInfo { session: shell.name(), host: "localhost".to_owned(), user: local_username(), profile: None };
            let auto_log = app.state::<SettingsStore>().get().logs.auto_local;
            Ok(Launch { log_info, auto_log, encoding: utf8, backend: Backend::Local(shell) })
        }
    }
}

/// A session's backend, with what it runs on.
enum Backend {
    Ssh { profile: SshProfile, route: Route, connections: Connections, carry: Vec<String> },
    Shared { connection: Arc<ssh::Connection>, connections: Connections },
    /// `password_of`: the saved session whose password answers the login prompt.
    Telnet { remote: Remote, route: Route, password_of: Option<String> },
    Serial(SerialOptions),
    Local(pty::Shell),
}

impl Backend {
    /// The task of session `id`. A shared connection learns of the session before the task
    /// runs, so that closing the session right away releases it.
    fn start(self, id: SessionId, io: TermIo) -> impl Future<Output = ()> + Send + 'static {
        if let Backend::Shared { connection, connections } = &self {
            connections.attach(id, connection.clone(), &io);
        }
        self.run(id, io)
    }

    async fn run(self, id: SessionId, io: TermIo) {
        match self {
            Backend::Ssh { profile, route, connections, carry } => ssh::run(profile, route, id, io, connections, carry).await,
            Backend::Shared { connection, .. } => ssh::run_shared(connection, io).await,
            Backend::Telnet { remote, route, password_of } => {
                // Looked up in the session's task when the server asks; `block_in_place` lets
                // the other tasks move to other threads while the keychain waits.
                let password = move || password_of.and_then(|id| tokio::task::block_in_place(|| secrets::get_password(&id)));
                telnet::run(remote, route, password, io).await
            }
            Backend::Serial(options) => serial::run(options, io).await,
            Backend::Local(shell) => pty::run(shell, io).await,
        }
    }
}

/// Starts logging a session by hand, in a new file; returns its path.
#[tauri::command]
pub async fn session_log_start(app: AppHandle, id: SessionId) -> Result<PathBuf> {
    blocking(app, move |app| {
        let sink = app.state::<SessionManager>().sink(id)?;
        let path = app.state::<Logs>().start(sink.log(), &app.state::<SettingsStore>().get().logs)?;
        sink.event(log_event(Ok(path.clone())));
        Ok(path)
    })
    .await
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
pub async fn logs_summary(app: AppHandle) -> Result<LogSummary> {
    blocking(app, |app| Ok(app.state::<Logs>().summary())).await
}

/// Where new logs go. Created only when `create` is set, to be shown in the file manager:
/// opening the settings leaves no empty folder behind for someone who never records a log.
#[tauri::command]
pub async fn logs_directory(app: AppHandle, create: bool) -> Result<PathBuf> {
    blocking(app, move |app| {
        let dir = app.state::<Logs>().directory(&app.state::<SettingsStore>().get().logs);
        if create {
            let _ = std::fs::create_dir_all(&dir);
        }
        Ok(dir)
    })
    .await
}

/// How many logs a saved session has.
#[tauri::command]
pub async fn logs_count(app: AppHandle, profile_id: String) -> Result<usize> {
    blocking(app, move |app| Ok(app.state::<Logs>().count(&profile_id))).await
}

/// Deletes the logs of a saved session, or all logs (`profile_id` absent), except those
/// being written; returns how many were deleted.
#[tauri::command]
pub async fn logs_delete(app: AppHandle, profile_id: Option<String>) -> Result<usize> {
    blocking(app, move |app| {
        let logs = app.state::<Logs>();
        Ok(match profile_id {
            Some(id) => logs.delete_for(&id),
            None => logs.delete_all(),
        })
    })
    .await
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
pub async fn serial_ports(app: AppHandle) -> Result<Vec<serial::PortInfo>> {
    blocking(app, |_| Ok(serial::ports())).await
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
pub fn session_close(sessions: State<'_, SessionManager>, id: SessionId) -> Result<()> {
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

/// Starts (or restarts with a new definition) a forwarding rule of the session's saved
/// session: where it runs (perhaps another tab's connection), else on the session's
/// connection. Progress is reported to the saved session's tabs as session events.
#[tauri::command]
pub fn forward_start(connections: State<'_, Connections>, id: SessionId, rule: ForwardRule) -> Result<()> {
    connections.start_forward(id, rule.normalize()?)
}

/// Stops a forwarding rule of the session's saved session, wherever it runs.
#[tauri::command]
pub fn forward_stop(connections: State<'_, Connections>, id: SessionId, rule_id: String) -> Result<()> {
    connections.stop_forward(id, &rule_id)
}

/// The forwarding rules that closing sessions `ids` would stop, although another tab of their
/// saved session stays connected and could keep them running.
#[tauri::command]
pub fn forward_keep_candidates(connections: State<'_, Connections>, ids: Vec<SessionId>) -> Vec<ForwardRule> {
    connections.forwards_to_keep(&ids)
}

/// Keeps the forwarding rules of sessions `ids` running on another tab of their saved session
/// once they close.
#[tauri::command]
pub fn forward_keep(connections: State<'_, Connections>, ids: Vec<SessionId>) {
    connections.keep_forwards(&ids);
}

/// The forwarding rules to start again when session `id` reconnects (see [`SessionSpec::Profile`]).
#[tauri::command]
pub fn forward_carry(connections: State<'_, Connections>, id: SessionId) -> Vec<String> {
    connections.forwards_to_carry(id)
}

/// Answers a ZMODEM download (`sz`): save into `dir`, or the download folder.
#[tauri::command]
pub async fn zmodem_save_to(app: AppHandle, id: SessionId, dir: Option<PathBuf>) -> Result<()> {
    blocking(app, move |app| {
        let dir = match dir {
            Some(dir) => dir,
            None => download_dir(app, &app.state::<SettingsStore>())?,
        };
        app.state::<SessionManager>().zmodem(id)?.reply(zmodem::Reply::Destination(dir));
        Ok(())
    })
    .await
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
