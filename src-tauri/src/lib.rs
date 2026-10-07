mod commands;
mod config;
mod error;
mod secrets;
mod session;
mod ssh;

use tauri::Manager;

use config::ProfileStore;
use session::SessionManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let profiles = app.path().app_config_dir()?.join("profiles.json");
            app.manage(ProfileStore::load(profiles)?);
            Ok(())
        })
        .manage(SessionManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::profiles_list,
            commands::profile_save,
            commands::profile_delete,
            commands::ssh_open,
            commands::session_write,
            commands::session_resize,
            commands::session_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
