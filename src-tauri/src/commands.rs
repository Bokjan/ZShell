use tauri::ipc::Channel;
use tauri::State;

use crate::error::Result;
use crate::session::{loopback, SessionId, SessionInput, SessionManager};

#[tauri::command]
pub fn session_open_loopback(sessions: State<'_, SessionManager>, on_output: Channel) -> SessionId {
    let (id, input) = sessions.register();
    tauri::async_runtime::spawn(loopback::run(on_output, input));
    id
}

#[tauri::command]
pub fn session_write(sessions: State<'_, SessionManager>, id: SessionId, data: String) -> Result<()> {
    sessions.send(id, SessionInput::Data(data.into_bytes()))
}

#[tauri::command]
pub fn session_resize(sessions: State<'_, SessionManager>, id: SessionId, cols: u16, rows: u16) -> Result<()> {
    sessions.send(id, SessionInput::Resize { cols, rows })
}

#[tauri::command]
pub fn session_close(sessions: State<'_, SessionManager>, id: SessionId) -> Result<()> {
    sessions.remove(id)
}
