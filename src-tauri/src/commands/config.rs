//! Commands for what is saved: settings, quick commands, sessions, folders, proxies,
//! known hosts, and importing and exporting sessions.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

use crate::backup;
use crate::config::{Additions, Folder, Item, Profile, ProfileStore};
use crate::error::{Error, Result};
use crate::forward::ForwardRule;
use crate::import;
use crate::logging::Logs;
use crate::persist::{SetAside, SetAsideFile};
use crate::proxy::Proxy;
use crate::quick::{QuickCommandStore, QuickCommands};
use crate::secrets;
use crate::settings::{Settings, SettingsStore};
use crate::ssh::{known_hosts, Connections};

use super::blocking;

#[tauri::command]
pub fn settings_get(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// Stores the settings; returns them as validated (e.g. with the font size clamped).
#[tauri::command]
pub fn settings_set(app: AppHandle, store: State<'_, SettingsStore>, settings: Settings) -> Result<Settings> {
    let settings = store.set(settings)?;
    // A shorter retention applies right away; checking every log can take a while.
    let keep_days = settings.logs.keep_days;
    tauri::async_runtime::spawn_blocking(move || app.state::<Logs>().clean_up(keep_days));
    Ok(settings)
}

/// The folder the sessions and settings are saved in; created if needed, to be shown in the
/// file manager.
#[tauri::command]
pub async fn config_directory(app: AppHandle) -> Result<PathBuf> {
    blocking(app, |app| {
        let dir = app.path().app_config_dir()?;
        let _ = std::fs::create_dir_all(&dir);
        Ok(dir)
    })
    .await
}

/// The data files that could not be read at startup and were set aside; reported once.
#[tauri::command]
pub fn config_set_aside(set_aside: State<'_, SetAside>) -> Vec<SetAsideFile> {
    set_aside.take()
}

#[tauri::command]
pub fn quick_commands_get(store: State<'_, QuickCommandStore>) -> QuickCommands {
    store.get()
}

/// Stores the quick commands; returns them as stored (new ones get ids).
#[tauri::command]
pub fn quick_commands_set(store: State<'_, QuickCommandStore>, commands: QuickCommands) -> Result<QuickCommands> {
    store.set(commands)
}

#[tauri::command]
pub fn profiles_list(store: State<'_, ProfileStore>) -> Vec<Profile> {
    store.list()
}

/// A saved session or proxy, with the error storing its password gave, if any. It is saved
/// either way: the dialog then goes on editing it, so that saving again doesn't add another.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Saved<T> {
    saved: T,
    password_error: Option<Error>,
}

/// `password`: `None` keeps the stored password, `Some("")` clears it.
#[tauri::command]
pub async fn profile_save(app: AppHandle, profile: Profile, password: Option<String>) -> Result<Saved<Profile>> {
    blocking(app, move |app| {
        let profile = app.state::<ProfileStore>().save(profile)?;
        // A session that is no longer SSH has no forwarding rules; running ones stop.
        let rules: Vec<String> = profile.forwards().iter().map(|rule| rule.id.clone()).collect();
        app.state::<Connections>().retain_forwards(&profile.id, &rules);
        let stored = match password.as_deref() {
            None => Ok(()),
            Some("") => secrets::delete_password(&profile.id),
            Some(password) => secrets::set_password(&profile.id, password),
        };
        Ok(Saved { saved: profile, password_error: stored.err().map(Error::from) })
    })
    .await
}

#[tauri::command]
pub fn profile_set_forwards(
    store: State<'_, ProfileStore>,
    connections: State<'_, Connections>,
    profile_id: String,
    forwards: Vec<ForwardRule>,
) -> Result<Profile> {
    let profile = store.set_forwards(&profile_id, forwards)?;
    // A rule deleted while running (perhaps on another tab's connection) stops.
    let ids: Vec<String> = profile.forwards().iter().map(|rule| rule.id.clone()).collect();
    connections.retain_forwards(&profile_id, &ids);
    Ok(profile)
}

#[tauri::command]
pub fn proxies_list(store: State<'_, ProfileStore>) -> Vec<Proxy> {
    store.proxies()
}

/// `password`: `None` keeps the stored password, `Some("")` clears it. A proxy without a user
/// name keeps none.
#[tauri::command]
pub async fn proxy_save(app: AppHandle, proxy: Proxy, password: Option<String>) -> Result<Saved<Proxy>> {
    blocking(app, move |app| {
        let store = app.state::<ProfileStore>();
        let had_password = store.proxy(&proxy.id).is_ok_and(|previous| previous.uses_password());
        let proxy = store.save_proxy(proxy)?;
        let stored = match password.as_deref() {
            // The keychain is only touched when there can be a password to remove.
            _ if !proxy.uses_password() => match had_password {
                true => secrets::delete_proxy_password(&proxy.id),
                false => Ok(()),
            },
            None => Ok(()),
            Some("") => secrets::delete_proxy_password(&proxy.id),
            Some(password) => secrets::set_proxy_password(&proxy.id, password),
        };
        Ok(Saved { saved: proxy, password_error: stored.err().map(Error::from) })
    })
    .await
}

/// Deletes a proxy no session uses (`proxy.inUse` otherwise), with its password; returns the
/// error removing the password gave, if any (see `profile_delete`).
#[tauri::command]
pub async fn proxy_delete(app: AppHandle, id: String) -> Result<Option<Error>> {
    blocking(app, move |app| {
        app.state::<ProfileStore>().delete_proxy(&id)?;
        Ok(secrets::delete_proxy_password(&id).err().map(Error::from))
    })
    .await
}

/// The default OpenSSH client config path (`~/.ssh/config`), whether or not it exists.
#[tauri::command]
pub fn ssh_config_default_path() -> Option<PathBuf> {
    import::default_path()
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
pub async fn ssh_config_import(app: AppHandle, path: PathBuf, aliases: Vec<String>) -> Result<Vec<Profile>> {
    blocking(app, move |app| {
        app.state::<ProfileStore>().add_all(|here| {
            let (proxies, profiles) = import::plan(&path, &aliases, here.profiles, here.proxies)?;
            Ok(Additions { proxies, profiles, ..Additions::default() })
        })
    })
    .await
}

#[tauri::command]
pub fn folders_list(store: State<'_, ProfileStore>) -> Vec<Folder> {
    store.folders()
}

/// Creates a folder (empty id) or renames one; returns it as saved.
#[tauri::command]
pub async fn folder_save(app: AppHandle, folder: Folder) -> Result<Folder> {
    blocking(app, move |app| app.state::<ProfileStore>().save_folder(folder)).await
}

/// Deletes a folder; its sessions and subfolders move up into its parent.
#[tauri::command]
pub async fn folder_delete(app: AppHandle, id: String) -> Result<()> {
    blocking(app, move |app| app.state::<ProfileStore>().delete_folder(&id)).await
}

/// Moves a session or folder into `parent` (`None`: top level), before `before` (a session or
/// folder of the same kind) or at the end.
#[tauri::command]
pub async fn tree_move(app: AppHandle, item: Item, parent: Option<String>, before: Option<String>) -> Result<()> {
    blocking(app, move |app| app.state::<ProfileStore>().move_item(item, parent, before)).await
}

/// Copies a session, with its saved password, as `name` right after it. The copy is made
/// even if the keychain refuses its password.
#[tauri::command]
pub async fn profile_duplicate(app: AppHandle, id: String, name: String) -> Result<Saved<Profile>> {
    blocking(app, move |app| {
        let copy = app.state::<ProfileStore>().duplicate(&id, &name)?;
        let stored = match secrets::get_password(&id) {
            Some(password) => secrets::set_password(&copy.id, &password),
            None => Ok(()),
        };
        Ok(Saved { saved: copy, password_error: stored.err().map(Error::from) })
    })
    .await
}

/// Writes all sessions, folders and proxies (without passwords) to `path`.
#[tauri::command]
pub async fn sessions_export(app: AppHandle, path: PathBuf) -> Result<()> {
    blocking(app, move |app| {
        let (profiles, folders, proxies) = app.state::<ProfileStore>().snapshot();
        backup::export(&path, folders, proxies, profiles)
    })
    .await
}

#[tauri::command]
pub async fn sessions_import_scan(app: AppHandle, path: PathBuf) -> Result<backup::Scan> {
    blocking(app, move |app| backup::scan(&path, &app.state::<ProfileStore>().list())).await
}

/// Imports the sessions `ids` (ids in the file) and what they need (jump hosts, proxies,
/// folders), if the file is still the one `sessions_import_scan` read (`digest`).
#[tauri::command]
pub async fn sessions_import(app: AppHandle, path: PathBuf, ids: Vec<String>, digest: String) -> Result<()> {
    blocking(app, move |app| app.state::<ProfileStore>().add_all(|here| backup::plan(&path, &ids, &digest, here)).map(drop)).await
}

/// Deletes a session with its password; returns the error removing the password gave, if
/// any. The session is deleted either way: failing would have the dialog try again or save
/// it back, so the password left behind is reported apart.
#[tauri::command]
pub async fn profile_delete(app: AppHandle, id: String) -> Result<Option<Error>> {
    blocking(app, move |app| {
        app.state::<ProfileStore>().delete(&id)?;
        Ok(secrets::delete_password(&id).err().map(Error::from))
    })
    .await
}
