//! Commands for the session logs on disk.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::Result;
use crate::logging::{LogSummary, Logs};
use crate::settings::SettingsStore;

use super::blocking;

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
