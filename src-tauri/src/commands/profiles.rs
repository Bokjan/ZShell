//! Commands for the saved sessions, their folders and forwarding rules, and the proxies.

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use ts_rs::TS;

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
        app.state::<Connections>().session_changed(&profile.id, Some(&profile));
        Ok(profile)
    })
    .await;
    notified(&window, Change::Profiles, result)
}

/// A forwarding rule just saved, with its session.
#[derive(Serialize, TS)]
pub struct SavedForward {
    pub profile: Profile,
    pub rule: ForwardRule,
}

/// Saves one forwarding rule of a session (see [`ProfileStore::save_forward`]).
#[tauri::command]
pub async fn profile_forward_save(app: AppHandle, window: WebviewWindow, profile_id: String, rule: ForwardRule) -> Result<SavedForward> {
    let result = blocking(app, move |app| app.state::<ProfileStore>().save_forward(&profile_id, rule)).await;
    let (profile, rule) = notified(&window, Change::Profiles, result)?;
    Ok(SavedForward { profile, rule })
}

/// Deletes one forwarding rule of a session; it stops if it runs (perhaps on another tab's
/// connection).
#[tauri::command]
pub async fn profile_forward_delete(app: AppHandle, window: WebviewWindow, profile_id: String, rule_id: String) -> Result<Profile> {
    let result = blocking(app, move |app| {
        let profile = app.state::<ProfileStore>().delete_forward(&profile_id, &rule_id)?;
        app.state::<Connections>().stop_saved_forwards(&profile_id, Some(&rule_id));
        Ok(profile)
    })
    .await;
    notified(&window, Change::Profiles, result)
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

/// Deletes a session, with its password; its forwarding rules running in its tabs stop (see
/// [`Connections::session_changed`]), which the frontend asked about first (`forward_sessions_of`).
#[tauri::command]
pub async fn profile_delete(app: AppHandle, window: WebviewWindow, id: String) -> Result<()> {
    let result = blocking(app, move |app| {
        app.state::<ProfileStore>().delete(&id)?;
        app.state::<Connections>().session_changed(&id, None);
        Ok(())
    })
    .await;
    notified(&window, Change::Profiles, result)
}
