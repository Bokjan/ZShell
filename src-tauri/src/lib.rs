mod commands;
mod config;
mod error;
mod secrets;
mod session;
mod sftp;
mod ssh;

use tauri::Manager;

use config::ProfileStore;
use session::SessionManager;
use sftp::transfer::Transfers;
use ssh::Connections;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let profiles = app.path().app_config_dir()?.join("profiles.json");
            app.manage(ProfileStore::load(profiles)?);
            Ok(())
        })
        .manage(SessionManager::default())
        .manage(Connections::default())
        .manage(Transfers::default())
        .invoke_handler(tauri::generate_handler![
            commands::profiles_list,
            commands::profile_save,
            commands::profile_delete,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
