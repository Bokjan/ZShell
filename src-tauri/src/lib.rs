// Every unsafe block says why it is sound; CI checks each platform's code (`-D warnings`).
#![warn(clippy::undocumented_unsafe_blocks)]
#[macro_use]
mod i18n;

mod applock;
mod backup;
mod bindings;
mod color_pick;
mod commands;
mod config;
mod connections;
mod crypto;
#[cfg(target_os = "linux")]
mod desktop;
mod encoding;
mod envelope;
mod error;
mod events;
mod forward;
#[cfg(windows)]
mod hidden_window;
mod import;
#[cfg(unix)]
mod instance;
mod launch;
mod local_name;
mod logging;
mod menu;
mod model;
mod net;
mod persist;
mod proxy;
mod pty;
mod quick;
mod screen_reader;
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

use tauri::Manager;

use applock::AppLock;
use config::ProfileStore;
use connections::Connections;
use logging::Logs;
use persist::SetAside;
use quick::QuickCommandStore;
use session::SessionManager;
use settings::SettingsStore;
use sftp::edit::Edits;
use sftp::transfer::Transfers;
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

/// A webview's page is gone (reloaded, or its window destroyed): what it started goes with it,
/// since nothing shows it or would ever close it. Its sessions close, its transfers are
/// cancelled and its edits stop being watched.
fn page_gone(app: &tauri::AppHandle, webview: &str) {
    app.state::<SessionManager>().remove_webview(webview);
    app.state::<Transfers>().cancel_webview(webview);
    // Managed in `setup`, which a page may not have reached.
    if let Some(edits) = app.try_state::<Edits>() {
        edits.stop_webview(webview);
    }
}

/// The folder the sessions and settings are saved in: `ZShell` in the system's configuration
/// folder (`~/Library/Application Support`, `%APPDATA%`, `~/.config`), named after the app as
/// people expect there, rather than after the bundle identifier as Tauri's `app_config_dir`.
/// The credential store's service, the caches and the web view's data keep the identifier.
pub(crate) fn config_dir(app: &tauri::AppHandle) -> tauri::Result<std::path::PathBuf> {
    Ok(app.path().config_dir()?.join("ZShell"))
}

/// Where downloaded files wait for another application to take them (editing in a local
/// editor, dragging out): the system's temporary folder, which is the user's own on macOS and
/// Windows. On Linux `/tmp` is shared by all users, so it is the app's cache folder there,
/// readable by the user only.
pub(crate) fn scratch_dir(app: &tauri::AppHandle) -> tauri::Result<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let dir = app.path().app_cache_dir()?;
        persist::secure_dir(&dir)?;
        Ok(dir)
    }
    #[cfg(not(target_os = "linux"))]
    app.path().temp_dir()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    instance::keep_activation_token();
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
    let builder = builder.menu(|app| menu::build(app, &[], &Default::default(), false));
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config_dir = config_dir(app.handle())?;
            // Before anything is read or written: another instance may have the folder.
            #[cfg(unix)]
            {
                // Neither stops the app: a file system without permissions (a FAT drive) or
                // locks (some network ones) is used as it is. The lock is tried whether or
                // not the folder could be made private.
                if let Err(e) = persist::secure_dir(&config_dir) {
                    eprintln!("can't make {} private: {e}", config_dir.display());
                }
                match instance::claim(&config_dir) {
                    Ok(instance::Claim::Ours(lock)) => {
                        app.manage(lock);
                        #[cfg(target_os = "linux")]
                        {
                            let handle = app.handle().clone();
                            instance::listen(&config_dir, move |token| window::come_forward(&handle, token));
                        }
                    }
                    Ok(instance::Claim::Taken(pid)) => {
                        // Nothing of this instance starts: it ends once it has handed over.
                        instance::hand_over(&config_dir, pid);
                        return Ok(());
                    }
                    Err(e) => eprintln!("can't lock {}, so another instance would go unnoticed: {e}", config_dir.display()),
                }
            }
            let set_aside = SetAside::default();
            // First, so that backend text is in the chosen language from the start.
            let settings = SettingsStore::load(config_dir.join("settings.json"), &set_aside);
            i18n::apply(&settings.get().language);
            app.manage(settings);
            // The one built before the language was known.
            #[cfg(target_os = "macos")]
            app.set_menu(menu::build(app.handle(), &[], &Default::default(), false)?)?;
            // Only reads the header of an encrypted vault, and doesn't make a missing one:
            // that may wait for the credential store, and the frontend shows the unlock screen
            // meanwhile.
            let vault = Arc::new(Vault::open(config_dir.clone(), Arc::new(secrets::SystemStore)));
            app.manage(ProfileStore::load(vault.clone()));
            app.manage(QuickCommandStore::load(vault.clone()));
            let handle = app.handle().clone();
            let app_lock = Arc::new(AppLock::for_platform(vault.clone(), move || {
                events::emit(&handle, events::AppLocked);
            }));
            applock::watch_idle(app_lock.clone());
            applock::watch_system(app_lock.clone());
            app.manage(app_lock);
            app.manage(vault);
            let documents = app.path().document_dir().unwrap_or_else(|_| config_dir.clone());
            app.manage(Logs::load(config_dir.join("logs.json"), documents.join("ZShellLogs"), &set_aside));
            app.manage(set_aside);
            clean_up_logs(app.handle().clone());
            app.manage(update::Updates::load(config_dir.join("update.json")));
            update::watch(app.handle().clone());
            screen_reader::watch(app.handle().clone());
            app.manage(Edits::new(scratch_dir(app.handle())?.join("ZShell-edit")));
            #[cfg(target_os = "linux")]
            {
                desktop::read_monospace_font_ahead();
                window::watch_buttons(app.handle());
                window::watch_theme(app.handle());
            }
            window::create_main(app)?;
            Ok(())
        })
        .on_menu_event(|app, event| menu::chosen(app, event.id().as_ref()))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => window::update_border(window),
            tauri::WindowEvent::Focused(true) => {
                window::focused(window);
                #[cfg(target_os = "macos")]
                menu::show_full_screen(window.app_handle());
            }
            tauri::WindowEvent::Destroyed => {
                // Its webview has the same label.
                page_gone(window.app_handle(), window.label());
                window::destroyed(window);
            }
            _ => {}
        })
        // A page that loads again (reloaded, or after its web content process crashed) has
        // none of the tabs the old one had: their sessions would run on unseen.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                page_gone(webview.app_handle(), webview.label());
                window::page_loading(webview.app_handle(), webview.label());
            }
        })
        .manage(SessionManager::default())
        .manage(Windows::default())
        .manage(menu::NativeMenu::default())
        .manage(tab_drag::TabDrag::default())
        .manage(Connections::default())
        .manage(Transfers::default())
        .invoke_handler(tauri::generate_handler![
            commands::locale_get,
            commands::menu::app_menu_set,
            commands::menu::app_menu_set_label,
            commands::update::update_status,
            commands::update::update_check,
            commands::settings::settings_get,
            commands::settings::system_monospace_font,
            commands::settings::screen_reader_running,
            commands::settings::settings_set,
            commands::settings::config_directory,
            commands::settings::config_set_aside,
            commands::settings::quick_commands_get,
            commands::settings::quick_commands_set,
            commands::profiles::profiles_list,
            commands::profiles::profile_save,
            commands::profiles::profile_forward_save,
            commands::profiles::profile_forward_delete,
            commands::profiles::profile_delete,
            commands::profiles::profile_duplicate,
            commands::profiles::proxies_list,
            commands::profiles::proxy_save,
            commands::profiles::proxy_delete,
            commands::profiles::folders_list,
            commands::profiles::tree_order,
            commands::profiles::folder_save,
            commands::profiles::folder_delete,
            commands::profiles::tree_move,
            commands::backup::sessions_export,
            commands::backup::sessions_import_scan,
            commands::backup::sessions_import,
            commands::vault::vault_status,
            commands::vault::vault_try_unlock,
            commands::vault::vault_unlock,
            commands::vault::vault_reset,
            commands::vault::vault_set_protection,
            commands::vault::credential_store_available,
            commands::applock::app_lock_status,
            commands::applock::app_lock_lock,
            commands::applock::app_lock_unlock,
            commands::applock::app_lock_configure,
            commands::local_username,
            commands::color_pick,
            commands::home_directory,
            commands::ssh_files::ssh_config_default_path,
            commands::ssh_files::ssh_default_key_path,
            commands::ssh_files::ssh_key_file_exists,
            commands::ssh_files::known_hosts_path,
            commands::ssh_files::known_hosts_list,
            commands::ssh_files::known_hosts_find,
            commands::ssh_files::known_hosts_remove,
            commands::ssh_files::ssh_config_scan,
            commands::ssh_files::ssh_config_import,
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
            commands::sftp::sftp_shell_dir,
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
            commands::window::window_open,
            commands::window::window_take,
            commands::window::window_close_empty,
            commands::window::window_list,
            commands::window::window_set_title,
            commands::window::window_send_tab,
            commands::window::window_pane_stayed,
            commands::tab_drag::window_set_tab_bar,
            commands::tab_drag::tab_drag_move,
            commands::tab_drag::tab_drag_end,
            commands::window::window_busy_reply,
            commands::window::app_busy_count,
            commands::window::app_quit,
            commands::window::app_bell,
            commands::window::window_title_double_click,
            commands::window::window_system_menu,
            commands::window::window_file_dialog,
            commands::window::window_set_drag_area,
            commands::window::window_buttons,
            commands::window::window_set_theme,
            commands::window::window_set_maximize_button,
            commands::window::window_set_snapped_border_color,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                quitting(app);
            }
        });
}

/// How long quitting waits for the session logs to be written to the end: until the local
/// shells that don't exit when their terminal closes have been killed, and a little more.
const LOGS_CLOSE_TIMEOUT: std::time::Duration = pty::RELEASED_WITHIN.saturating_add(std::time::Duration::from_millis(300));

/// The app is about to end: the sessions close as if their pages were gone, and their logs
/// are written to the end, which would otherwise lose what they buffer (up to a second of
/// output) with the process.
fn quitting(app: &tauri::AppHandle) {
    for label in app.webview_windows().into_keys() {
        page_gone(app, &label);
    }
    // Managed in `setup`, which may not have got that far.
    if let Some(logs) = app.try_state::<Logs>() {
        logs.wait_all_closed(LOGS_CLOSE_TIMEOUT);
    }
}
