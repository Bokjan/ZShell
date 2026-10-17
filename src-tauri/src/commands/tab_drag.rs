//! Commands for dragging tabs out of a window (see `tab_drag/`).

use tauri::{AppHandle, State, WebviewWindow};

use crate::error::Result;

use crate::tab_drag::{self, DropTarget, Rect, TabDrag};

/// See [`tab_drag::set_tab_bar`].
#[tauri::command]
pub fn window_set_tab_bar(window: WebviewWindow, drag: State<'_, TabDrag>, rect: Rect) {
    tab_drag::set_tab_bar(window, drag.inner(), rect)
}

/// See [`tab_drag::pointer_moved`].
#[tauri::command]
pub async fn tab_drag_move(app: AppHandle, window: WebviewWindow, id: String, outside: bool) -> Result<DropTarget> {
    Ok(tab_drag::pointer_moved(app, window, id, outside).await?)
}

/// See [`tab_drag::end`].
#[tauri::command]
pub async fn tab_drag_end(
    app: AppHandle,
    window: WebviewWindow,
    drag: State<'_, TabDrag>,
    id: String,
    dropped: bool,
    outside: bool,
) -> Result<Option<DropTarget>> {
    Ok(tab_drag::end(app, window, drag.inner(), id, dropped, outside).await?)
}
