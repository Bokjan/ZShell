//! Commands for the app lock (see `applock.rs`). They wait on the blocking pool: the system's
//! dialog blocks until the user answers, and asking Windows Hello what it offers takes a moment.

use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::applock::{AppLock, AppLockSettings, AppLockStatus, Trigger};
use crate::error::Result;

use super::blocking;

#[tauri::command]
pub async fn app_lock_status(app: AppHandle) -> Result<AppLockStatus> {
    blocking(app, |app| Ok(app.state::<Arc<AppLock>>().status())).await
}

/// Locks now (the "Lock" action), if the app lock is on.
#[tauri::command]
pub async fn app_lock_lock(app: AppHandle) -> Result<AppLockStatus> {
    blocking(app, |app| {
        let lock = app.state::<Arc<AppLock>>();
        lock.lock(Trigger::Manual);
        Ok(lock.status())
    })
    .await
}

/// Asks the user to authenticate, over the window that asks; `appLock.cancelled` if they
/// didn't.
#[tauri::command]
pub async fn app_lock_unlock(app: AppHandle, window: WebviewWindow) -> Result<AppLockStatus> {
    blocking(app, move |app| app.state::<Arc<AppLock>>().unlock(Some(&window))).await
}

/// Saves the settings; turning the lock on asks the user to authenticate first.
#[tauri::command]
pub async fn app_lock_configure(app: AppHandle, window: WebviewWindow, settings: AppLockSettings) -> Result<AppLockStatus> {
    blocking(app, move |app| app.state::<Arc<AppLock>>().configure(settings, Some(&window))).await
}
