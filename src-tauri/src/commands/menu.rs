//! Commands for the app menu (see `menu.rs`).

use tauri::{AppHandle, State, WebviewWindow};

use crate::menu::{self, NativeMenu, Section};

/// See [`menu::set`].
#[tauri::command]
pub fn app_menu_set(app: AppHandle, menu: State<NativeMenu>, sections: Vec<Section>) -> tauri::Result<()> {
    menu::set(app, menu.inner(), sections)
}

/// See [`menu::set_label`].
#[tauri::command]
pub fn app_menu_set_label(app: AppHandle, window: WebviewWindow, menu: State<NativeMenu>, id: String, label: String) {
    menu::set_label(app, window, menu.inner(), id, label)
}
