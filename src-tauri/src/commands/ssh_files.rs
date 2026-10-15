//! Commands for OpenSSH's files: known hosts, key files and importing `~/.ssh/config`.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::config::{Additions, ProfileStore};
use crate::error::Result;
use crate::import;
use crate::model::Profile;
use crate::ssh::{self, known_hosts};

use super::{blocking, notified, Change};

/// The default OpenSSH client config path (`~/.ssh/config`), whether or not it exists.
#[tauri::command]
pub fn ssh_config_default_path() -> Option<PathBuf> {
    import::default_path()
}

/// The key file a new session set to use one starts with: the first of OpenSSH's default
/// keys (`~/.ssh/id_ed25519` and so on) that exists; `None` if there is none.
#[tauri::command]
pub async fn ssh_default_key_path() -> Option<String> {
    tauri::async_runtime::spawn_blocking(ssh::default_key_path).await.ok().flatten()
}

/// Whether the key file at `path` (`~/…` included) exists.
#[tauri::command]
pub async fn ssh_key_file_exists(path: String) -> bool {
    tauri::async_runtime::spawn_blocking(move || ssh::key_file_exists(&path)).await.unwrap_or(false)
}

/// `~/.ssh/known_hosts`, whether or not it exists.
#[tauri::command]
pub fn known_hosts_path() -> Option<PathBuf> {
    known_hosts::path()
}

/// The host keys in known_hosts; empty if there is no file.
#[tauri::command]
pub async fn known_hosts_list(app: AppHandle) -> Result<Vec<known_hosts::Entry>> {
    blocking(app, |_| known_hosts::list()).await
}

/// The lines of the entries for a host (`host`, `host:port` or `[host]:port`), including
/// hashed ones.
#[tauri::command]
pub async fn known_hosts_find(app: AppHandle, query: String) -> Result<Vec<usize>> {
    blocking(app, move |_| known_hosts::find(&query)).await
}

/// Removes the entry on `line`, if the line still reads `text`.
#[tauri::command]
pub async fn known_hosts_remove(app: AppHandle, line: usize, text: String) -> Result<()> {
    blocking(app, move |_| known_hosts::remove(line, &text)).await
}

#[tauri::command]
pub async fn ssh_config_scan(app: AppHandle, path: PathBuf) -> Result<Vec<import::Candidate>> {
    blocking(app, move |app| import::scan(&path, &app.state::<ProfileStore>().list())).await
}

/// Imports the selected hosts (and the jump hosts and proxy commands they need); returns the
/// new profiles.
#[tauri::command]
pub async fn ssh_config_import(app: AppHandle, window: WebviewWindow, path: PathBuf, aliases: Vec<String>) -> Result<Vec<Profile>> {
    let result = blocking(app, move |app| {
        app.state::<ProfileStore>().add_all(|here| {
            let (proxies, profiles) = import::plan(&path, &aliases, here.profiles, here.proxies)?;
            Ok(Additions { proxies, profiles, ..Additions::default() })
        })
    })
    .await;
    notified(&window, Change::Profiles, result)
}
