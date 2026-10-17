//! Commands for the app's settings and the quick commands, and the files they are kept in.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::error::Result;
use crate::logging::Logs;
use crate::model::QuickCommands;
use crate::model::Settings;
use crate::persist::{SetAside, SetAsideFile};
use crate::quick::QuickCommandStore;
use crate::settings::SettingsStore;

use super::{blocking, notified, Change};

#[tauri::command]
pub fn settings_get(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// The desktop's monospace font, which terminals use before the built-in defaults: GNOME's on
/// Linux, `None` elsewhere. It may ask the settings portal, so off the async threads.
#[tauri::command]
pub async fn system_monospace_font() -> Option<String> {
    #[cfg(target_os = "linux")]
    return tauri::async_runtime::spawn_blocking(crate::desktop::current_monospace_font).await.ok().flatten();
    #[cfg(not(target_os = "linux"))]
    None
}

/// Whether the system's screen reader is running; then `screen-reader-changed` tells.
#[tauri::command]
pub fn screen_reader_running() -> bool {
    crate::screen_reader::running()
}

/// Stores the settings; returns them as validated (e.g. with the font size clamped). Off the
/// main thread, as writing the file waits for the disk: the frontend sends one request at a
/// time, which keeps them in order (see `latestSave`).
#[tauri::command]
pub async fn settings_set(app: AppHandle, window: WebviewWindow, settings: Settings) -> Result<Settings> {
    let stored = blocking(app.clone(), move |app| {
        app.state::<SettingsStore>().set(settings, |settings| {
            crate::i18n::apply(&settings.language);
        })
    })
    .await;
    let settings = notified(&window, Change::Settings, stored)?;
    // A shorter retention applies right away; checking every log can take a while.
    let keep_days = settings.logs.keep_days;
    tauri::async_runtime::spawn_blocking(move || app.state::<Logs>().clean_up(keep_days));
    Ok(settings)
}

/// The folder the sessions and settings are saved in; created if needed, to be shown in the
/// file manager.
#[tauri::command]
pub async fn config_directory(app: AppHandle) -> Result<PathBuf> {
    blocking(app, |app| {
        let dir = crate::config_dir(app)?;
        let _ = std::fs::create_dir_all(&dir);
        Ok(dir)
    })
    .await
}

/// The data files that could not be read at startup and were set aside; reported once.
#[tauri::command]
pub fn config_set_aside(set_aside: State<'_, SetAside>) -> Vec<SetAsideFile> {
    set_aside.take()
}

#[tauri::command]
pub fn quick_commands_get(store: State<'_, QuickCommandStore>) -> QuickCommands {
    store.get()
}

/// Stores the quick commands; returns them as stored (new ones get ids). Off the main thread,
/// one request at a time, as [`settings_set`].
#[tauri::command]
pub async fn quick_commands_set(app: AppHandle, window: WebviewWindow, commands: QuickCommands) -> Result<QuickCommands> {
    let stored = blocking(app, move |app| app.state::<QuickCommandStore>().set(commands)).await;
    notified(&window, Change::QuickCommands, stored)
}
