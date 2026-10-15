//! Commands for the saved sessions, their folders and forwarding rules, and the proxies.

use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::config::ProfileStore;
use crate::connections::Connections;
use crate::error::Result;
use crate::model::ForwardRule;
use crate::model::Proxy;
use crate::model::{Folder, Item, Profile};

use super::{blocking, notified, Change};

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

#[tauri::command]
pub fn folders_list(store: State<'_, ProfileStore>) -> Vec<Folder> {
    store.folders()
}

/// The order the sessions and folders are shown in, mixed in each folder.
#[tauri::command]
pub fn tree_order(store: State<'_, ProfileStore>) -> Vec<Item> {
    store.order()
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
/// folder) or at the end.
#[tauri::command]
pub async fn tree_move(app: AppHandle, window: WebviewWindow, item: Item, parent: Option<String>, before: Option<Item>) -> Result<()> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().move_item(item, parent, before)).await;
    notified(&window, Change::Profiles, result)
}

/// Copies a session, with its saved password, as `name` right after it.
#[tauri::command]
pub async fn profile_duplicate(app: AppHandle, window: WebviewWindow, id: String, name: String) -> Result<Profile> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().duplicate(&id, &name)).await;
    notified(&window, Change::Profiles, result)
}

/// Deletes a session, with its password.
#[tauri::command]
pub async fn profile_delete(app: AppHandle, window: WebviewWindow, id: String) -> Result<()> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().delete(&id)).await;
    notified(&window, Change::Profiles, result)
}
