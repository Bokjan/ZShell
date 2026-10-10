#[macro_use]
mod i18n;

mod backup;
mod bindings;
mod commands;
mod config;
mod encoding;
mod error;
mod forward;
mod import;
mod local_name;
mod logging;
mod net;
mod persist;
mod proxy;
mod pty;
mod quick;
mod secrets;
mod serial;
mod session;
mod settings;
mod sftp;
mod ssh;
mod telnet;
mod window;
mod zmodem;

use tauri::{Emitter, Manager};

use config::ProfileStore;
use logging::Logs;
use persist::SetAside;
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
/// users expect it, and a File menu with "Close" (⌘W: the focused pane of a split tab, else
/// the tab) and "Close Window" (⇧⌘W), as in iTerm2; the predefined "Close Window" item would
/// take ⌘W.
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
            // Off the async threads: on a network drive that sleeps or is gone, the file
            // system calls block, and would hold up the sessions sharing those threads.
            let app = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || app.state::<Logs>().clean_up(keep_days)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before AppKit reads it.
    #[cfg(target_os = "macos")]
    disable_press_and_hold();
    let builder = tauri::Builder::default();
    // Opening the app again on Windows starts another process, and two would overwrite each
    // other's saved sessions and settings: bring the running one forward instead, as macOS
    // does by itself. Registered first, so that the second process ends before anything else.
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show_main(app)));
    // Tauri only gives macOS a default menu; other platforms keep having none.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let set_aside = SetAside::default();
            app.manage(ProfileStore::load(config_dir.join("profiles.json"), &set_aside));
            app.manage(SettingsStore::load(config_dir.join("settings.json"), &set_aside));
            app.manage(QuickCommandStore::load(config_dir.join("commands.json"), &set_aside));
            let documents = app.path().document_dir().unwrap_or_else(|_| config_dir.clone());
            app.manage(Logs::load(config_dir.join("logs.json"), documents.join("ZShellLogs"), &set_aside));
            app.manage(set_aside);
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
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => window::update_border(window),
            // Its webview has the same label.
            tauri::WindowEvent::Destroyed => window.state::<SessionManager>().remove_webview(window.label()),
            _ => {}
        })
        // A page that loads again (reloaded, or after its web content process crashed) has
        // none of the tabs the old one had: their sessions would run on unseen.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                webview.state::<SessionManager>().remove_webview(webview.label());
            }
        })
        .manage(SessionManager::default())
        .manage(Connections::default())
        .manage(Transfers::default())
        .invoke_handler(tauri::generate_handler![
            commands::set_locale,
            commands::config::settings_get,
            commands::config::settings_set,
            commands::config::config_directory,
            commands::config::config_set_aside,
            commands::config::quick_commands_get,
            commands::config::quick_commands_set,
            commands::config::profiles_list,
            commands::config::profile_save,
            commands::config::profile_set_forwards,
            commands::config::profile_delete,
            commands::config::profile_duplicate,
            commands::config::proxies_list,
            commands::config::proxy_save,
            commands::config::proxy_delete,
            commands::config::folders_list,
            commands::config::folder_save,
            commands::config::folder_delete,
            commands::config::tree_move,
            commands::config::sessions_export,
            commands::config::sessions_import_scan,
            commands::config::sessions_import,
            commands::local_username,
            commands::home_directory,
            commands::config::ssh_config_default_path,
            commands::config::known_hosts_path,
            commands::config::known_hosts_list,
            commands::config::known_hosts_find,
            commands::config::known_hosts_remove,
            commands::config::ssh_config_scan,
            commands::config::ssh_config_import,
            commands::session::session_open,
            commands::session::local_shell_name,
            commands::session::session_log_start,
            commands::session::session_log_stop,
            commands::logs::logs_summary,
            commands::logs::logs_directory,
            commands::logs::logs_count,
            commands::logs::logs_delete,
            commands::session::session_write,
            commands::session::session_resize,
            commands::session::session_break,
            commands::session::serial_ports,
            commands::session::session_ack,
            commands::session::session_foreground,
            commands::session::session_close,
            commands::sftp::sftp_open,
            commands::sftp::sftp_list,
            commands::sftp::sftp_mkdir,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_remove,
            commands::sftp::sftp_chmod,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_download,
            commands::sftp::sftp_download_as,
            commands::sftp::downloads_directory,
            commands::sftp::sftp_edit_open,
            commands::sftp::sftp_edit_reopen,
            commands::sftp::sftp_edit_upload,
            commands::sftp::sftp_edit_stop,
            commands::sftp::sftp_drag_out,
            commands::sftp::transfer_cancel,
            commands::forward::forward_start,
            commands::forward::forward_stop,
            commands::forward::forward_keep_candidates,
            commands::forward::forward_keep,
            commands::forward::forward_carry,
            commands::zmodem::zmodem_save_to,
            commands::zmodem::zmodem_send_files,
            commands::zmodem::zmodem_cancel,
            window::window_title_double_click,
            window::window_system_menu,
            window::window_set_maximize_button,
            window::window_set_snapped_border_color,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
