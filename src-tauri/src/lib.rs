#[macro_use]
mod i18n;

mod commands;
mod config;
mod error;
mod forward;
mod import;
mod pty;
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
const LOCAL_TERMINAL_MENU_ID: &str = "new-local-terminal";

/// The default macOS menu, plus "Settings…" (⌘,) in the app menu, where Mac users expect
/// it, and "New Local Terminal" in the File menu. The settings shortcut has to be a menu
/// item: macOS handles ⌘, before the web view sees it. Elsewhere the frontend handles
/// Ctrl+, itself.
#[cfg(target_os = "macos")]
fn app_menu<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(tauri::menu::MenuItemKind::Submenu(app_submenu)) = menu.items()?.first() {
        let settings = tauri::menu::MenuItem::with_id(app, SETTINGS_MENU_ID, t!("menu.settings"), true, Some("CmdOrCtrl+,"))?;
        // After "About ZShell" and its separator.
        app_submenu.insert(&settings, 2)?;
        app_submenu.insert(&tauri::menu::PredefinedMenuItem::separator(app)?, 3)?;
    }
    if let Some(tauri::menu::MenuItemKind::Submenu(file_submenu)) = menu.items()?.get(1) {
        let local = tauri::menu::MenuItem::with_id(app, LOCAL_TERMINAL_MENU_ID, t!("menu.newLocalTerminal"), true, None::<&str>)?;
        // Before "Close Window".
        file_submenu.insert(&local, 0)?;
        file_submenu.insert(&tauri::menu::PredefinedMenuItem::separator(app)?, 1)?;
    }
    Ok(menu)
}

/// Holding a key should repeat it, as in other terminals, rather than open the accent
/// picker (macOS "press and hold"; arrow keys repeat either way, having no accents). Set as
/// a registration default, so `defaults write org.boyin.zshell ApplePressAndHoldEnabled
/// -bool true` still brings the picker back.
#[cfg(target_os = "macos")]
fn disable_press_and_hold() {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSUserDefaults};

    let key = NSString::from_str("ApplePressAndHoldEnabled");
    let value = NSNumber::new_bool(false);
    let defaults: objc2::rc::Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[&*key], &[value.as_ref() as &AnyObject]);
    // SAFETY: the dictionary holds only property list objects (an NSString key and an
    // NSNumber value), as registerDefaults requires.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before AppKit reads it.
    #[cfg(target_os = "macos")]
    disable_press_and_hold();
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
            } else if event.id() == LOCAL_TERMINAL_MENU_ID {
                let _ = app.emit("open-local-terminal", ());
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
            commands::local_open,
            commands::local_shell_name,
            commands::session_write,
            commands::session_resize,
            commands::session_ack,
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
