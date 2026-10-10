//! Commands answering a ZMODEM transfer's questions (where to save, what to send).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::session::{SessionId, SessionManager};
use crate::settings::SettingsStore;
use crate::zmodem;

use super::blocking;

/// Answers a ZMODEM download (`sz`): save into `dir`, or the download folder.
#[tauri::command]
pub async fn zmodem_save_to(app: AppHandle, id: SessionId, dir: Option<PathBuf>) -> Result<()> {
    blocking(app, move |app| {
        let dir = match dir {
            Some(dir) => dir,
            None => super::sftp::download_dir(app, &app.state::<SettingsStore>())?,
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
