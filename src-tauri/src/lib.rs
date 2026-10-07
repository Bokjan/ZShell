#[macro_use]
mod i18n;

mod commands;
mod config;
mod error;
mod forward;
mod import;
mod secrets;
mod session;
mod settings;
mod sftp;
mod ssh;

use tauri::{Emitter, Manager};

use config::ProfileStore;
use session::SessionManager;
use settings::SettingsStore;
use sftp::transfer::Transfers;
use ssh::Connections;

const SETTINGS_MENU_ID: &str = "settings";

/// The default macOS menu, plus "Settings…" (⌘,) in the app menu, where Mac users expect
/// it. The shortcut has to be a menu item there: macOS handles ⌘, before the web view sees
/// it. Elsewhere the frontend handles Ctrl+, itself.
#[cfg(target_os = "macos")]
fn app_menu<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(tauri::menu::MenuItemKind::Submenu(app_submenu)) = menu.items()?.first() {
        let settings = tauri::menu::MenuItem::with_id(app, SETTINGS_MENU_ID, t!("menu.settings"), true, Some("CmdOrCtrl+,"))?;
        // After "About ZShell" and its separator.
        app_submenu.insert(&settings, 2)?;
        app_submenu.insert(&tauri::menu::PredefinedMenuItem::separator(app)?, 3)?;
    }
    Ok(menu)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // Tauri only gives macOS a default menu; other platforms keep having none.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            app.manage(ProfileStore::load(config_dir.join("profiles.json"))?);
            app.manage(SettingsStore::load(config_dir.join("settings.json")));
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == SETTINGS_MENU_ID {
                let _ = app.emit("open-settings", ());
            }
        })
        .manage(SessionManager::default())
        .manage(Connections::default())
        .manage(Transfers::default())
        .invoke_handler(tauri::generate_handler![
            commands::set_locale,
            commands::settings_get,
            commands::settings_set,
            commands::profiles_list,
            commands::profile_save,
            commands::profile_set_forwards,
            commands::profile_delete,
            commands::ssh_config_default_path,
            commands::ssh_config_scan,
            commands::ssh_config_import,
            commands::ssh_open,
            commands::session_write,
            commands::session_resize,
            commands::session_close,
            commands::sftp_open,
            commands::sftp_list,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_chmod,
            commands::sftp_upload,
            commands::sftp_download,
            commands::transfer_cancel,
            commands::forward_start,
            commands::forward_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
