//! Tauri commands, by area. What the commands share: running blocking work off the main
//! thread, and the few that belong to no area.

// Commands take injected state plus IPC arguments, so long parameter lists are expected.
#![allow(clippy::too_many_arguments)]

pub mod config;
pub mod forward;
pub mod logs;
pub mod session;
pub mod sftp;
pub mod zmodem;

use std::path::PathBuf;

use tauri::AppHandle;

use crate::error::Result;
use crate::i18n;

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

/// The user's home folder, which `~` stands for in key paths (and in `~/.ssh`).
#[tauri::command]
pub fn home_directory() -> Option<PathBuf> {
    std::env::home_dir()
}

/// The user name `ssh` uses when none is given.
#[tauri::command]
pub fn local_username() -> String {
    crate::config::local_username()
}
