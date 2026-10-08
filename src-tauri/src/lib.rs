#[macro_use]
mod i18n;

mod backup;
mod commands;
mod config;
mod encoding;
mod error;
mod forward;
mod import;
mod logging;
mod pty;
mod quick;
mod secrets;
mod session;
mod settings;
mod sftp;
mod ssh;
mod window;
mod zmodem;

use tauri::{Emitter, Manager};

use config::ProfileStore;
use logging::Logs;
use quick::QuickCommandStore;
use session::SessionManager;
use settings::SettingsStore;
use sftp::edit::Edits;
use sftp::transfer::Transfers;
use ssh::Connections;

const SETTINGS_MENU_ID: &str = "settings";
const CLOSE_TAB_MENU_ID: &str = "close-tab";
const CLOSE_WINDOW_MENU_ID: &str = "close-window";
const QUIT_MENU_ID: &str = "quit";

/// The macOS menu: Tauri's default one, with "Settings…" (⌘,) in the app menu, where Mac
/// users expect it, and a File menu with "Close Tab" (⌘W) and "Close Window" (⇧⌘W), as in
/// Terminal.app; the predefined "Close Window" item would take ⌘W.
/// These shortcuts have to be menu items: macOS handles them before the web view sees them.
/// "Quit" closes the window rather than exiting directly, so the frontend can ask first when
/// tabs are connected. Elsewhere the frontend handles Ctrl+, and Ctrl+Shift+W itself.
#[cfg(target_os = "macos")]
fn app_menu<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID};

    let info = app.package_info();
    let about = AboutMetadata { name: Some(info.name.clone()), version: Some(info.version.to_string()), ..Default::default() };
    let separator = || PredefinedMenuItem::separator(app);
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about))?,
                    &separator()?,
                    &MenuItem::with_id(app, SETTINGS_MENU_ID, t!("menu.settings"), true, Some("CmdOrCtrl+,"))?,
                    &separator()?,
                    &PredefinedMenuItem::services(app, None)?,
                    &separator()?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &separator()?,
                    &MenuItem::with_id(app, QUIT_MENU_ID, t!("menu.quit", name = info.name), true, Some("CmdOrCtrl+Q"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                t!("menu.file"),
                true,
                &[
                    &MenuItem::with_id(app, CLOSE_TAB_MENU_ID, t!("menu.closeTab"), true, Some("CmdOrCtrl+W"))?,
                    &MenuItem::with_id(app, CLOSE_WINDOW_MENU_ID, t!("menu.closeWindow"), true, Some("CmdOrCtrl+Shift+W"))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                t!("menu.edit"),
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &separator()?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(app, t!("menu.view"), true, &[&PredefinedMenuItem::fullscreen(app, None)?])?,
            &Submenu::with_id_and_items(
                app,
                WINDOW_SUBMENU_ID,
                t!("menu.window"),
                true,
                &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, None)?],
            )?,
            &Submenu::with_id_and_items(app, HELP_SUBMENU_ID, t!("menu.help"), true, &[])?,
        ],
    )
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

/// Deletes logs past the retention in the settings, now and every few hours.
fn clean_up_logs(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(6 * 3600));
        loop {
            timer.tick().await;
            let keep_days = app.state::<SettingsStore>().get().logs.keep_days;
            app.state::<Logs>().clean_up(keep_days);
        }
    });
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
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            app.manage(ProfileStore::load(config_dir.join("profiles.json"))?);
            app.manage(SettingsStore::load(config_dir.join("settings.json")));
            app.manage(QuickCommandStore::load(config_dir.join("commands.json")));
            let documents = app.path().document_dir().unwrap_or_else(|_| config_dir.clone());
            app.manage(Logs::load(config_dir.join("logs.json"), documents.join("ZShellLogs")));
            clean_up_logs(app.handle().clone());
            app.manage(Edits::new(app.path().temp_dir()?.join("ZShell-edit")));
            window::create_main(app)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == SETTINGS_MENU_ID {
                let _ = app.emit("open-settings", ());
            } else if event.id() == CLOSE_TAB_MENU_ID {
                // The frontend may ask first, and closes the window when there are no tabs.
                let _ = app.emit("close-tab", ());
            } else if event.id() == CLOSE_WINDOW_MENU_ID || event.id() == QUIT_MENU_ID {
                // The last window closing quits the app; the frontend may ask first.
                match app.get_webview_window("main") {
                    Some(window) => {
                        let _ = window.close();
                    }
                    None => app.exit(0),
                }
            }
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                window::update_border(window);
            }
        })
        .manage(SessionManager::default())
        .manage(Connections::default())
        .manage(Transfers::default())
        .invoke_handler(tauri::generate_handler![
            commands::set_locale,
            commands::settings_get,
            commands::settings_set,
            commands::quick_commands_get,
            commands::quick_commands_set,
            commands::profiles_list,
            commands::profile_save,
            commands::profile_set_forwards,
            commands::profile_delete,
            commands::profile_duplicate,
            commands::folders_list,
            commands::folder_save,
            commands::folder_delete,
            commands::tree_move,
            commands::sessions_export,
            commands::sessions_import_scan,
            commands::sessions_import,
            commands::ssh_quick_open,
            commands::local_username,
            commands::ssh_config_default_path,
            commands::ssh_config_scan,
            commands::ssh_config_import,
            commands::ssh_open,
            commands::ssh_open_shared,
            commands::local_open,
            commands::local_shell_name,
            commands::session_log_start,
            commands::session_log_stop,
            commands::logs_summary,
            commands::logs_directory,
            commands::logs_count,
            commands::logs_delete,
            commands::session_write,
            commands::session_resize,
            commands::session_ack,
            commands::session_foreground,
            commands::session_close,
            commands::sftp_open,
            commands::sftp_list,
            commands::sftp_mkdir,
            commands::sftp_rename,
            commands::sftp_remove,
            commands::sftp_chmod,
            commands::sftp_upload,
            commands::sftp_download,
            commands::sftp_download_as,
            commands::downloads_directory,
            commands::sftp_edit_open,
            commands::sftp_edit_reopen,
            commands::sftp_edit_upload,
            commands::sftp_edit_stop,
            commands::sftp_drag_out,
            commands::transfer_cancel,
            commands::forward_start,
            commands::forward_stop,
            commands::zmodem_save_to,
            commands::zmodem_send_files,
            commands::zmodem_cancel,
            window::window_title_double_click,
            window::window_system_menu,
            window::window_set_maximize_button,
            window::window_set_snapped_border_color,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
