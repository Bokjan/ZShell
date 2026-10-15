//! Commands for checking for updates (see `update.rs`).

use tauri::AppHandle;

use crate::error::Result;
use crate::update::{self, Status, Updates};

/// See [`update::status`].
#[tauri::command]
pub fn update_status(updates: tauri::State<Updates>) -> Status {
    update::status(updates.inner())
}

/// See [`update::check`].
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Status> {
    update::check(app).await
}
