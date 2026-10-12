#[macro_use]
mod i18n;

mod applock;
mod backup;
mod bindings;
mod commands;
mod config;
mod crypto;
mod encoding;
mod error;
mod forward;
mod import;
#[cfg(unix)]
mod instance;
mod local_name;
mod logging;
mod menu;
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
mod tab_drag;
mod telnet;
mod update;
mod vault;
mod window;
mod zmodem;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use applock::AppLock;
use config::ProfileStore;
use logging::Logs;
use persist::SetAside;
use quick::QuickCommandStore;
use session::SessionManager;
use settings::SettingsStore;
use sftp::edit::Edits;
use sftp::transfer::Transfers;
use ssh::Connections;
use vault::Vault;
use window::Windows;

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
    // other's saved sessions and settings: bring the running one forward instead. Registered
    // first, so that the second process ends before anything else. Elsewhere the lock is the
    // configuration folder's (`instance.rs`).
    #[cfg(windows)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show_current(app)));
    // Tauri only gives macOS a default menu; other platforms keep having none.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(|app| menu::build(app, &[], &Default::default()));
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            // Before anything is read or written: another instance may have the folder.
            #[cfg(unix)]
            if persist::secure_dir(&config_dir).is_ok() {
                match instance::claim(&config_dir) {
                    Ok(instance::Claim::Ours(lock)) => {
                        app.manage(lock);
                    }
                    Ok(instance::Claim::Taken(pid)) => {
                        // Nothing of this instance starts: it ends once it has handed over.
                        instance::hand_over(pid);
                        return Ok(());
                    }
                    Err(_) => {}
                }
            }
            let set_aside = SetAside::default();
            // Only reads the header of an encrypted vault: unlocking it may wait for the
            // credential store, and the frontend shows the unlock screen meanwhile.
            let vault = Arc::new(Vault::open(config_dir.clone(), Arc::new(secrets::SystemStore), &set_aside));
            vault::install(vault.clone());
            app.manage(ProfileStore::load(vault.clone()));
            app.manage(QuickCommandStore::load(vault.clone()));
            let handle = app.handle().clone();
            let app_lock = Arc::new(AppLock::for_platform(vault.clone(), move || {
                let _ = handle.emit("app-locked", ());
            }));
            applock::watch_idle(app_lock.clone());
            applock::watch_system(app_lock.clone());
            app.manage(app_lock);
            app.manage(vault);
            app.manage(SettingsStore::load(config_dir.join("settings.json"), &set_aside));
            let documents = app.path().document_dir().unwrap_or_else(|_| config_dir.clone());
            app.manage(Logs::load(config_dir.join("logs.json"), documents.join("ZShellLogs"), &set_aside));
            app.manage(set_aside);
            clean_up_logs(app.handle().clone());
            app.manage(update::Updates::load(config_dir.join("update.json")));
            update::watch(app.handle().clone());
            app.manage(Edits::new(app.path().temp_dir()?.join("ZShell-edit")));
            window::create_main(app)?;
            Ok(())
        })
        .on_menu_event(|app, event| menu::chosen(app, event.id().as_ref()))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => window::update_border(window),
            tauri::WindowEvent::Focused(true) => window::focused(window),
            tauri::WindowEvent::Destroyed => {
                // Its webview has the same label.
                window.state::<SessionManager>().remove_webview(window.label());
                window::destroyed(window);
            }
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
        .manage(Windows::default())
        .manage(menu::NativeMenu::default())
        .manage(tab_drag::TabDrag::default())
        .manage(Connections::default())
        .manage(Transfers::default())
        .invoke_handler(tauri::generate_handler![
            commands::set_locale,
            menu::app_menu_set,
            menu::app_menu_set_label,
            update::update_status,
            update::update_check,
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
            commands::vault::vault_status,
            commands::vault::vault_try_unlock,
            commands::vault::vault_unlock,
            commands::vault::vault_reset,
            commands::vault::vault_set_protection,
            commands::vault::vault_plain_copies,
            commands::vault::vault_delete_plain_copies,
            commands::vault::credential_store_available,
            commands::applock::app_lock_status,
            commands::applock::app_lock_lock,
            commands::applock::app_lock_unlock,
            commands::applock::app_lock_configure,
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
            commands::session::session_write_bytes,
            commands::session::session_resize,
            commands::session::session_break,
            commands::session::serial_ports,
            commands::session::session_ack,
            commands::session::session_adopt,
            commands::session::session_handover,
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
            window::window_open,
            window::window_take,
            window::window_close_empty,
            window::window_list,
            window::window_set_title,
            window::window_send_tab,
            window::window_pane_stayed,
            tab_drag::window_set_tab_bar,
            tab_drag::tab_drag_move,
            tab_drag::tab_drag_end,
            window::window_busy_reply,
            window::app_busy_count,
            window::app_quit,
            window::window_title_double_click,
            window::window_system_menu,
            window::window_set_maximize_button,
            window::window_set_snapped_border_color,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
