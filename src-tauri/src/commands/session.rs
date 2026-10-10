//! Commands for terminal sessions: opening one (what it runs, with its log) and passing
//! the terminal's input, size and acknowledgements to it.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};
use ts_rs::TS;

use crate::config::{Connection, Profile, ProfileStore, Protocol, Remote, SerialOptions, SshOptions, SshProfile};
use crate::encoding;
use crate::error::{Error, Result};
use crate::logging::{LogInfo, LogOpen, LogSlot, Logs};
use crate::net::Route;
use crate::pty;
use crate::secrets;
use crate::serial;
use crate::session::{SessionEvent, SessionId, SessionInput, SessionManager, TermIo};
use crate::settings::SettingsStore;
use crate::ssh::{self, Connections};
use crate::telnet;

use super::{blocking, local_username};

/// Starts a new session's log (see [`Logs::open`]) on the blocking pool.
async fn open_log(app: &AppHandle, slot: &Arc<LogSlot>, how: Option<LogOpen>, auto: bool) -> Result<Option<Result<PathBuf>>> {
    let slot = slot.clone();
    blocking(app.clone(), move |app| {
        let settings = app.state::<SettingsStore>().get().logs;
        Ok(app.state::<Logs>().open(&slot, how.unwrap_or_default(), auto, &settings))
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
    let page = sessions.page(webview.label());
    let Launch { log_info, auto_log, encoding, backend } = launch(&app, spec)?;
    let slot = LogSlot::new(log_info);
    let opened = open_log(&app, &slot, log, auto_log).await?;
    let id = sessions.spawn(&page, channel, (cols, rows), slot, encoding, |id, io| backend.start(id, io))?;
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
                _ if !crate::ssh::known_hosts::is_plain_host(&host) => return Err(Error::new("profile.invalidHost")),
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
