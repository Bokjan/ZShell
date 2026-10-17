//! Commands for the windows: opening one, moving tabs between them, quitting, and the title bar and window buttons drawn by the page (see `window/`).

use tauri::{AppHandle, State, WebviewWindow};

use crate::error::Result;
use crate::session::{SessionId, SessionManager};
use crate::tab_drag::Dropped;
use crate::window::{self, Rect, WindowButton, WindowContent, WindowInfo, Windows};

/// See [`window::open`].
#[tauri::command]
pub async fn window_open(
    app: AppHandle,
    window: WebviewWindow,
    windows: State<'_, Windows>,
    session_manager: State<'_, SessionManager>,
    content: WindowContent,
    sessions: Vec<SessionId>,
    dropped: Option<Dropped>,
) -> Result<String> {
    window::open(app, window, windows.inner(), session_manager.inner(), content, sessions, dropped).await
}

/// See [`window::take`].
#[tauri::command]
pub fn window_take(window: WebviewWindow, windows: State<'_, Windows>) -> Vec<WindowContent> {
    window::take(window, windows.inner())
}

/// See [`window::close_empty`].
#[tauri::command]
pub fn window_close_empty(app: AppHandle, window: WebviewWindow, windows: State<'_, Windows>) -> Result<bool> {
    Ok(window::close_empty(app, window, windows.inner())?)
}

/// See [`window::list`].
#[tauri::command]
pub fn window_list(app: AppHandle, window: WebviewWindow) -> Vec<WindowInfo> {
    window::list(app, window)
}

/// See [`window::set_title`].
#[tauri::command]
pub fn window_set_title(app: AppHandle, window: WebviewWindow, title: String) -> Result<()> {
    Ok(window::set_title(app, window, title)?)
}

/// See [`window::send_tab`].
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn window_send_tab(
    app: AppHandle,
    window: WebviewWindow,
    windows: State<'_, Windows>,
    session_manager: State<'_, SessionManager>,
    target: String,
    tab: serde_json::Value,
    sessions: Vec<SessionId>,
    at: Option<f64>,
) -> Result<()> {
    window::send_tab(app, window, windows.inner(), session_manager.inner(), target, tab, sessions, at)
}

/// See [`window::pane_stayed`].
#[tauri::command]
pub fn window_pane_stayed(app: AppHandle, target: String, pane: u32) {
    window::pane_stayed(app, target, pane)
}

/// See [`window::busy_count`].
#[tauri::command]
pub async fn app_busy_count(app: AppHandle, windows: State<'_, Windows>, sessions: State<'_, SessionManager>) -> Result<u32> {
    Ok(window::busy_count(app, windows.inner(), sessions.inner()).await?)
}

/// See [`window::busy_reply`].
#[tauri::command]
pub fn window_busy_reply(window: WebviewWindow, windows: State<'_, Windows>, query: u64, count: u32) {
    window::busy_reply(window, windows.inner(), query, count)
}

/// See [`window::bell`].
#[tauri::command]
pub fn app_bell(app: AppHandle) {
    window::bell(app)
}

/// See [`window::quit`].
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    window::quit(app)
}

/// See [`window::title_double_click`].
#[tauri::command]
pub fn window_title_double_click(window: WebviewWindow) -> Result<()> {
    Ok(window::title_double_click(window)?)
}

/// See [`window::system_menu`].
#[tauri::command]
pub fn window_system_menu(window: WebviewWindow) -> Result<()> {
    Ok(window::system_menu(window)?)
}

/// See [`window::file_dialog`].
#[tauri::command]
pub fn window_file_dialog(window: WebviewWindow, open: bool) -> Result<()> {
    Ok(window::file_dialog(window, open)?)
}

/// See [`window::set_drag_area`].
#[tauri::command]
pub fn window_set_drag_area(window: WebviewWindow, rect: Option<Rect>) -> Result<()> {
    Ok(window::set_drag_area(window, rect)?)
}

/// See [`window::buttons`].
#[tauri::command]
pub fn window_buttons() -> Vec<WindowButton> {
    window::buttons()
}

/// See [`window::set_theme`].
#[tauri::command]
pub async fn window_set_theme(window: WebviewWindow, theme: Option<tauri::Theme>) -> Result<()> {
    Ok(window::set_theme(window, theme).await?)
}

/// See [`window::set_snapped_border_color`].
#[tauri::command]
pub fn window_set_snapped_border_color(window: WebviewWindow, color: String) -> Result<()> {
    Ok(window::set_snapped_border_color(window, color)?)
}

/// See [`window::set_maximize_button`].
#[tauri::command]
pub fn window_set_maximize_button(window: WebviewWindow, rect: Option<Rect>) -> Result<()> {
    Ok(window::set_maximize_button(window, rect)?)
}
