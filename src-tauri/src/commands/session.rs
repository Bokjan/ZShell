//! Commands for terminal sessions: opening one (what it runs, with its log), passing the
//! terminal's input, size and acknowledgements to it, and moving it to another window.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};

use crate::config::ProfileStore;
use crate::connections::Connections;
use crate::error::Result;
use crate::events;
use crate::launch::{self, Launch, SessionSpec, Sources};
use crate::logging::{LogOpen, LogSlot, Logs};
use crate::pty;
use crate::serial;
use crate::session::{SessionEvent, SessionId, SessionInput, SessionManager, SessionSnapshot};
use crate::settings::SettingsStore;

use super::blocking;

/// Starts a new session's log (see [`Logs::open`]) on the blocking pool.
async fn open_log(app: &AppHandle, slot: &Arc<LogSlot>, how: Option<LogOpen>, auto: bool) -> Result<Option<Result<PathBuf>>> {
    let slot = slot.clone();
    blocking(app.clone(), move |app| {
        let settings = app.state::<SettingsStore>().get().logs;
        Ok(app.state::<Logs>().open(&slot, how.unwrap_or_default(), auto, &settings))
    })
    .await
}

/// Tells a new session's tab, in `webview`, about the log it started with, or why that failed.
fn report_log(sessions: &SessionManager, webview: &str, id: SessionId, opened: Option<Result<PathBuf>>) {
    if let (Some(opened), Ok(sink)) = (opened, sessions.sink(webview, id)) {
        sink.event(log_event(opened));
    }
}

fn log_event(result: Result<PathBuf>) -> SessionEvent {
    match result {
        Ok(path) => SessionEvent::Log { path: Some(path.display().to_string()), error: None },
        Err(error) => SessionEvent::Log { path: None, error: Some(error) },
    }
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
    let local_shell = match spec {
        SessionSpec::Local => Some(tauri::async_runtime::spawn_blocking(pty::default_shell).await?),
        _ => None,
    };
    let sources = Sources {
        profiles: &app.state::<ProfileStore>(),
        connections: &app.state::<Connections>(),
        sessions: &sessions,
        settings: &app.state::<SettingsStore>(),
        local_shell,
    };
    let Launch { log_info, auto_log, encoding, backend } = launch::launch(spec, webview.label(), sources)?;
    let slot = LogSlot::new(log_info);
    let opened = open_log(&app, &slot, log, auto_log).await?;
    let id = sessions.spawn(&page, channel, (cols, rows), slot, encoding, |id, io| backend.start(id, io))?;
    report_log(&sessions, webview.label(), id, opened);
    Ok(id)
}

/// Starts logging a session by hand, in a new file; returns its path.
#[tauri::command]
pub async fn session_log_start(app: AppHandle, webview: Webview, id: SessionId) -> Result<PathBuf> {
    let webview = webview.label().to_owned();
    blocking(app, move |app| {
        let sink = app.state::<SessionManager>().sink(&webview, id)?;
        let path = app.state::<Logs>().start(sink.log(), &app.state::<SettingsStore>().get().logs)?;
        sink.event(log_event(Ok(path.clone())));
        Ok(path)
    })
    .await
}

#[tauri::command]
pub fn session_log_stop(webview: Webview, sessions: State<'_, SessionManager>, logs: State<'_, Logs>, id: SessionId) -> Result<()> {
    let sink = sessions.sink(webview.label(), id)?;
    logs.stop(sink.log());
    sink.event(SessionEvent::Log { path: None, error: None });
    Ok(())
}

/// Short name of the default local shell, e.g. "zsh" or "pwsh"; none when the system doesn't
/// allow local terminals (Windows in S mode). Off the main thread: on Windows, finding it asks
/// the system's policy and looks through `PATH`, which may hold network drives.
#[tauri::command]
pub async fn local_shell_name() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| pty::local_shells_allowed().then(|| pty::default_shell().name())).await.ok().flatten()
}

#[tauri::command]
pub fn session_write(webview: Webview, sessions: State<'_, SessionManager>, id: SessionId, data: String) -> Result<()> {
    sessions.send(webview.label(), id, SessionInput::Data(data.into_bytes()))
}

/// Sends bytes that are not text (see [`SessionInput::Raw`]).
#[tauri::command]
pub fn session_write_bytes(webview: Webview, sessions: State<'_, SessionManager>, id: SessionId, data: Vec<u8>) -> Result<()> {
    sessions.send(webview.label(), id, SessionInput::Raw(data))
}

/// Sends a break (serial and Telnet sessions).
#[tauri::command]
pub fn session_break(webview: Webview, sessions: State<'_, SessionManager>, id: SessionId) -> Result<()> {
    sessions.send(webview.label(), id, SessionInput::Break)
}

/// The serial ports on this computer.
#[tauri::command]
pub async fn serial_ports(app: AppHandle) -> Result<Vec<serial::PortInfo>> {
    blocking(app, |_| Ok(serial::ports())).await
}

#[tauri::command]
pub fn session_resize(webview: Webview, sessions: State<'_, SessionManager>, id: SessionId, cols: u16, rows: u16) -> Result<()> {
    sessions.send(webview.label(), id, SessionInput::Resize { cols, rows })
}

/// Records that the frontend has processed `bytes` of the session's output.
#[tauri::command]
pub fn session_ack(webview: Webview, sessions: State<'_, SessionManager>, id: SessionId, bytes: usize) -> Result<()> {
    sessions.ack(webview.label(), id, bytes)
}

/// Moves a session offered to this window to a tab here (see [`SessionManager::adopt`]): its
/// output and events come on `channel` from now on. The tab that showed it hands over its screen
/// ([`session_handover`]) once it hears that the session moved, with `move_id`.
#[tauri::command]
pub fn session_adopt(
    webview: Webview,
    sessions: State<'_, SessionManager>,
    id: SessionId,
    channel: Channel,
    move_id: String,
) -> Result<()> {
    sessions.adopt(id, &sessions.page(webview.label()), channel, move_id)
}

/// Passes the screen of a session that moved away from this window to the window showing it
/// now, as the `session-snapshot` event.
#[tauri::command]
pub fn session_handover(app: AppHandle, webview: Webview, sessions: State<'_, SessionManager>, snapshot: SessionSnapshot) -> Result<()> {
    let owner = sessions.handover(webview.label(), snapshot.id, &snapshot.move_id)?;
    events::emit_to(&app, owner, events::SessionSnapshot(snapshot));
    Ok(())
}

/// The program running in the session's terminal other than the shell (an empty string if
/// its name is unknown), or `None`. Only local terminals can tell. Off the main thread: on
/// Windows, it is found in a snapshot of all the system's processes.
#[tauri::command]
pub async fn session_foreground(app: AppHandle, webview: Webview, id: SessionId) -> Result<Option<String>> {
    let label = webview.label().to_owned();
    blocking(app, move |app| app.state::<SessionManager>().foreground(&label, id)).await
}

#[tauri::command]
pub fn session_close(webview: Webview, sessions: State<'_, SessionManager>, id: SessionId) -> Result<()> {
    sessions.remove(webview.label(), id)
}
