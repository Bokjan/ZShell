//! Tauri commands, by area: every command the frontend can call is here, and only here, so
//! that the IPC surface can be read in one place (and `lib.rs` registers it). A command is a
//! thin entry point: it checks who calls (the session's window), passes the work to its
//! module, and tells the other windows what changed. What the commands share: running
//! blocking work off the main thread, and the few that belong to no area.
//!
//! State: a command takes the managed state it uses as `State` parameters; work moved to the
//! blocking pool (see [`blocking`]), which may outlive the call, reaches it through the
//! `AppHandle` instead.

// Commands take injected state plus IPC arguments, so long parameter lists are expected.
#![allow(clippy::too_many_arguments)]

pub mod applock;
pub mod backup;
pub mod forward;
pub mod logs;
pub mod menu;
pub mod profiles;
pub mod session;
pub mod settings;
pub mod sftp;
pub mod ssh_files;
pub mod tab_drag;
pub mod update;
pub mod vault;
pub mod window;
pub mod zmodem;

use std::path::PathBuf;

use tauri::{AppHandle, WebviewWindow};

use crate::error::Result;
use crate::events;
use crate::i18n;

/// The languages in effect: the backend's follows the `language` setting.
#[tauri::command]
pub fn locale_get() -> i18n::Locales {
    i18n::locales()
}

/// Runs a command's blocking work (files, the keychain, the system's device list) on the
/// blocking pool. Sync commands run on the main thread, where waiting freezes the window: on a
/// slow or network disk, or while macOS asks about each keychain entry after an update.
///
/// Async commands may run in any order, though: the frontend sends those replacing a whole
/// value (the settings, the quick commands) one at a time (see `latestSave`).
async fn blocking<T: Send + 'static>(app: AppHandle, work: impl FnOnce(&AppHandle) -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app)).await?
}

pub use crate::events::Change;

/// Tells the windows other than `from` about `change` if `result` is a success; returns it.
fn notified<T>(from: &WebviewWindow, change: Change, result: Result<T>) -> Result<T> {
    if result.is_ok() {
        events::tell_others(from, events::ConfigChanged(change));
    }
    result
}

/// The user's home folder, which `~` stands for in key paths (and in `~/.ssh`).
#[tauri::command]
pub fn home_directory() -> Option<PathBuf> {
    std::env::home_dir()
}

/// See [`crate::color_pick::pick`].
#[tauri::command]
pub async fn color_pick(window: WebviewWindow) -> Result<Option<String>> {
    crate::color_pick::pick(window).await
}

/// The user name `ssh` uses when none is given.
#[tauri::command]
pub fn local_username() -> String {
    crate::model::local_username()
}
