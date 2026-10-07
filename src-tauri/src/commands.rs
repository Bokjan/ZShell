use tauri::ipc::Channel;
use tauri::State;

use crate::config::{Profile, ProfileStore};
use crate::error::Result;
use crate::secrets;
use crate::session::{SessionEvent, SessionId, SessionInput, SessionManager};
use crate::ssh;

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
pub fn profile_delete(store: State<'_, ProfileStore>, id: String) -> Result<()> {
    store.delete(&id)?;
    secrets::delete_password(&id)?;
    Ok(())
}

#[tauri::command]
pub fn ssh_open(
    store: State<'_, ProfileStore>,
    sessions: State<'_, SessionManager>,
    profile_id: String,
    cols: u16,
    rows: u16,
    on_output: Channel,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId> {
    let profile = store.get(&profile_id)?;
    Ok(sessions.spawn(on_output, on_event, (cols, rows), |io| ssh::run(profile, io)))
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
