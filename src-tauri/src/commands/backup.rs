//! Commands for exporting sessions to a file and importing them from one (see `backup.rs`).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::backup;
use crate::config::{Export, ProfileStore};
use crate::error::Result;

use super::{blocking, notified, Change};

/// Writes all sessions, folders and proxies to `path`; with their saved passwords if
/// `passphrase` is given, the whole file encrypted with it.
#[tauri::command]
pub async fn sessions_export(app: AppHandle, path: PathBuf, passphrase: Option<String>) -> Result<()> {
    blocking(app, move |app| {
        let passphrase = passphrase.filter(|p| !p.is_empty());
        let Export { profiles, folders, proxies, order, passwords } = app.state::<ProfileStore>().export(passphrase.is_some())?;
        let passwords = passphrase.as_deref().zip(passwords);
        backup::export(&path, backup::Contents { folders, proxies, profiles, order }, passwords)
    })
    .await
}

/// What importing the file would add; an encrypted file takes its `passphrase`
/// (`import.passphraseRequired` without one, `import.wrongPassphrase` for another one).
#[tauri::command]
pub async fn sessions_import_scan(app: AppHandle, path: PathBuf, passphrase: Option<String>) -> Result<backup::Scan> {
    blocking(app, move |app| {
        let store = app.state::<ProfileStore>();
        backup::scan(&path, passphrase.as_deref(), &store.list(), &store.proxies())
    })
    .await
}

/// Imports the sessions `ids` (ids in the file) and what they need (jump hosts, proxies,
/// folders), with their passwords, if the file is still the one `sessions_import_scan` read
/// (`digest`).
#[tauri::command]
pub async fn sessions_import(
    app: AppHandle,
    window: WebviewWindow,
    path: PathBuf,
    ids: Vec<String>,
    digest: String,
    passphrase: Option<String>,
) -> Result<()> {
    let result = blocking(app, move |app| {
        app.state::<ProfileStore>().add_all(|here| backup::plan(&path, passphrase.as_deref(), &ids, &digest, here))?;
        Ok(())
    })
    .await;
    notified(&window, Change::Profiles, result)
}
