//! Commands for the app lock (see `applock.rs`). They wait on the blocking pool: the system's
//! dialog blocks until the user answers, and asking Windows Hello what it offers takes a moment.

use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::applock::{AppLock, AppLockSettings, AppLockStatus, Trigger};
use crate::error::Result;

use super::{blocking, notified, tell_others, Change};

#[tauri::command]
pub async fn app_lock_status(app: AppHandle) -> Result<AppLockStatus> {
    blocking(app, |app| Ok(app.state::<Arc<AppLock>>().status())).await
}

/// Locks now (the "Lock" action), if the app lock is on; the other windows hear `app-locked`,
/// as when the backend locks.
#[tauri::command]
pub async fn app_lock_lock(app: AppHandle, window: WebviewWindow) -> Result<AppLockStatus> {
    let status = blocking(app, |app| {
        let lock = app.state::<Arc<AppLock>>();
        lock.lock(Trigger::Manual);
        Ok(lock.status())
    })
    .await?;
    if status.locked {
        tell_others(&window, "app-locked", ());
    }
    Ok(status)
}

/// Asks the user to authenticate, over the window that asks; `appLock.cancelled` if they
/// didn't. `anyway`: once told that there is no way to authenticate left (`appLock.gone`),
/// unlocks without.
#[tauri::command]
pub async fn app_lock_unlock(app: AppHandle, window: WebviewWindow, anyway: bool) -> Result<AppLockStatus> {
    let asking = window.clone();
    let result = blocking(app, move |app| {
        let lock = app.state::<Arc<AppLock>>();
        if anyway {
            lock.unlock_anyway(Some(&asking))
        } else {
            lock.unlock(Some(&asking))
        }
    })
    .await;
    notified(&window, Change::AppLock, result)
}

/// Saves the settings; turning the lock on asks the user to authenticate first.
#[tauri::command]
pub async fn app_lock_configure(app: AppHandle, window: WebviewWindow, settings: AppLockSettings) -> Result<AppLockStatus> {
    let asking = window.clone();
    let result = blocking(app, move |app| app.state::<Arc<AppLock>>().configure(settings, Some(&asking))).await;
    notified(&window, Change::AppLock, result)
}
