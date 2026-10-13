//! Commands for what is saved: settings, quick commands, sessions, folders, proxies,
//! known hosts, and importing and exporting sessions. The vault's own are in `vault/`.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::backup;
use crate::config::{Additions, Folder, Item, Profile, ProfileStore};
use crate::error::Result;
use crate::forward::ForwardRule;
use crate::import;
use crate::logging::Logs;
use crate::persist::{SetAside, SetAsideFile};
use crate::proxy::Proxy;
use crate::quick::{QuickCommandStore, QuickCommands};
use crate::settings::{Settings, SettingsStore};
use crate::ssh::{known_hosts, Connections};
use crate::vault::Vault;

use super::{blocking, notified, Change};

#[tauri::command]
pub fn settings_get(store: State<'_, SettingsStore>) -> Settings {
    store.get()
}

/// The desktop's monospace font, which terminals use before the built-in defaults: GNOME's on
/// Linux, `None` elsewhere. It may ask the settings portal, so off the async threads.
#[tauri::command]
pub async fn system_monospace_font() -> Option<String> {
    #[cfg(target_os = "linux")]
    return tauri::async_runtime::spawn_blocking(crate::desktop::monospace_font).await.ok().flatten();
    #[cfg(not(target_os = "linux"))]
    None
}

/// Stores the settings; returns them as validated (e.g. with the font size clamped).
#[tauri::command]
pub fn settings_set(app: AppHandle, window: WebviewWindow, store: State<'_, SettingsStore>, settings: Settings) -> Result<Settings> {
    let settings = notified(&window, Change::Settings, store.set(settings))?;
    crate::i18n::apply(&settings.language);
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
        let dir = crate::config_dir(app)?;
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
pub fn quick_commands_set(window: WebviewWindow, store: State<'_, QuickCommandStore>, commands: QuickCommands) -> Result<QuickCommands> {
    notified(&window, Change::QuickCommands, store.set(commands))
}

#[tauri::command]
pub fn profiles_list(store: State<'_, ProfileStore>) -> Vec<Profile> {
    store.list()
}

/// `password`: `None` keeps the saved password, `Some("")` deletes it.
#[tauri::command]
pub async fn profile_save(app: AppHandle, window: WebviewWindow, profile: Profile, password: Option<String>) -> Result<Profile> {
    let result = blocking(app, move |app| {
        let profile = app.state::<ProfileStore>().save(profile, password.as_deref())?;
        // A session that is no longer SSH has no forwarding rules; running ones stop.
        let rules: Vec<String> = profile.forwards().iter().map(|rule| rule.id.clone()).collect();
        app.state::<Connections>().retain_forwards(&profile.id, &rules);
        Ok(profile)
    })
    .await;
    notified(&window, Change::Profiles, result)
}

#[tauri::command]
pub fn profile_set_forwards(
    window: WebviewWindow,
    store: State<'_, ProfileStore>,
    connections: State<'_, Connections>,
    profile_id: String,
    forwards: Vec<ForwardRule>,
) -> Result<Profile> {
    let profile = notified(&window, Change::Profiles, store.set_forwards(&profile_id, forwards))?;
    // A rule deleted while running (perhaps on another tab's connection) stops.
    let ids: Vec<String> = profile.forwards().iter().map(|rule| rule.id.clone()).collect();
    connections.retain_forwards(&profile_id, &ids);
    Ok(profile)
}

#[tauri::command]
pub fn proxies_list(store: State<'_, ProfileStore>) -> Vec<Proxy> {
    store.proxies()
}

/// `password`: `None` keeps the saved password, `Some("")` deletes it. A proxy without a user
/// name keeps none.
#[tauri::command]
pub async fn proxy_save(app: AppHandle, window: WebviewWindow, proxy: Proxy, password: Option<String>) -> Result<Proxy> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().save_proxy(proxy, password.as_deref())).await;
    notified(&window, Change::Profiles, result)
}

/// Deletes a proxy no session uses (`proxy.inUse` otherwise), with its password.
#[tauri::command]
pub async fn proxy_delete(app: AppHandle, window: WebviewWindow, id: String) -> Result<()> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().delete_proxy(&id)).await;
    notified(&window, Change::Profiles, result)
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

#[tauri::command]
pub fn folders_list(store: State<'_, ProfileStore>) -> Vec<Folder> {
    store.folders()
}

/// Creates a folder (empty id) or renames one; returns it as saved.
#[tauri::command]
pub async fn folder_save(app: AppHandle, window: WebviewWindow, folder: Folder) -> Result<Folder> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().save_folder(folder)).await;
    notified(&window, Change::Profiles, result)
}

/// Deletes a folder; its sessions and subfolders move up into its parent.
#[tauri::command]
pub async fn folder_delete(app: AppHandle, window: WebviewWindow, id: String) -> Result<()> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().delete_folder(&id)).await;
    notified(&window, Change::Profiles, result)
}

/// Moves a session or folder into `parent` (`None`: top level), before `before` (a session or
/// folder of the same kind) or at the end.
#[tauri::command]
pub async fn tree_move(app: AppHandle, window: WebviewWindow, item: Item, parent: Option<String>, before: Option<String>) -> Result<()> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().move_item(item, parent, before)).await;
    notified(&window, Change::Profiles, result)
}

/// Copies a session, with its saved password, as `name` right after it.
#[tauri::command]
pub async fn profile_duplicate(app: AppHandle, window: WebviewWindow, id: String, name: String) -> Result<Profile> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().duplicate(&id, &name)).await;
    notified(&window, Change::Profiles, result)
}

/// Writes all sessions, folders and proxies to `path`; with their saved passwords if
/// `passphrase` is given, the whole file encrypted with it.
#[tauri::command]
pub async fn sessions_export(app: AppHandle, path: PathBuf, passphrase: Option<String>) -> Result<()> {
    blocking(app, move |app| {
        let (profiles, folders, proxies) = app.state::<ProfileStore>().snapshot();
        let passwords = match passphrase.as_deref() {
            None | Some("") => None,
            Some(passphrase) => Some((passphrase, app.state::<Arc<Vault>>().data()?.passwords)),
        };
        backup::export(&path, folders, proxies, profiles, passwords)
    })
    .await
}

/// What importing the file would add; an encrypted file takes its `passphrase`
/// (`import.passphraseRequired` without one, `import.wrongPassphrase` for another one).
#[tauri::command]
pub async fn sessions_import_scan(app: AppHandle, path: PathBuf, passphrase: Option<String>) -> Result<backup::Scan> {
    blocking(app, move |app| backup::scan(&path, passphrase.as_deref(), &app.state::<ProfileStore>().list())).await
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

/// Deletes a session, with its password.
#[tauri::command]
pub async fn profile_delete(app: AppHandle, window: WebviewWindow, id: String) -> Result<()> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().delete(&id)).await;
    notified(&window, Change::Profiles, result)
}
