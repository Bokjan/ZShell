//! Dragging a tab out of its window (see ARCHITECTURE.md, "多窗口"): released over another
//! window's tab bar, the tab moves there; released anywhere else outside its window, it opens
//! in a window of its own under the cursor. The page drags with mouse events, as when it
//! reorders tabs, and asks here where the tab would go ([`tab_drag_move`]): the window under
//! the cursor is the one the system shows on top there, which only native code can tell, and
//! the cursor is read here in screen pixels, which the page's `screenX` doesn't give reliably
//! across displays with different scales.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};
use tokio::sync::oneshot;
use ts_rs::TS;

/// A rectangle in a page's CSS pixels.
#[derive(Clone, Copy, Deserialize, TS)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// A point in a page's CSS pixels.
#[derive(Clone, Copy, Deserialize, TS)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Where a dragged tab would go if released now.
#[derive(Clone, PartialEq, Debug, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DropTarget {
    /// Over the window it is dragged from.
    Here,
    /// Over window `label`'s tab bar, `x` CSS pixels from the left of its page.
    Window { label: String, x: f64 },
    /// Anywhere else (outside the windows, or over another window but not its tab bar).
    Elsewhere,
}

#[derive(Default)]
pub struct TabDrag {
    /// Where each window's tab bar is (see [`window_set_tab_bar`]).
    tab_bars: Mutex<HashMap<String, Rect>>,
    /// The window showing where a dragged tab would go in its tab bar.
    showing: Mutex<Option<String>>,
}

/// After a window is gone.
pub fn forget(app: &AppHandle, label: &str) {
    let drag = app.state::<TabDrag>();
    drag.tab_bars.lock().unwrap().remove(label);
    let mut showing = drag.showing.lock().unwrap();
    if showing.as_deref() == Some(label) {
        *showing = None;
    }
}

/// Records where this window's tab bar is, for tabs dragged from other windows.
#[tauri::command]
pub fn window_set_tab_bar(window: WebviewWindow, drag: State<'_, TabDrag>, rect: Rect) {
    drag.tab_bars.lock().unwrap().insert(window.label().to_owned(), rect);
}

/// Where a tab dragged from this window would go if released now. The window it would move
/// into shows where (the `tab-drag-over` event with the cursor's `x` in its page, then null
/// once it no longer would).
#[tauri::command]
pub async fn tab_drag_move(app: AppHandle, window: WebviewWindow) -> tauri::Result<DropTarget> {
    let target = drop_target(&app, window.label().to_owned()).await?;
    show(&app, &target);
    Ok(target)
}

/// Ends dragging a tab from this window: where it goes if `dropped` (rather than cancelled).
#[tauri::command]
pub async fn tab_drag_end(app: AppHandle, window: WebviewWindow, dropped: bool) -> tauri::Result<Option<DropTarget>> {
    show(&app, &DropTarget::Here);
    if !dropped {
        return Ok(None);
    }
    drop_target(&app, window.label().to_owned()).await.map(Some)
}

/// Shows where `target` would put the tab, in the window it would go into, and stops showing
/// it in the window that showed it before.
fn show(app: &AppHandle, target: &DropTarget) {
    let drag = app.state::<TabDrag>();
    let mut showing = drag.showing.lock().unwrap();
    let (label, x) = match target {
        DropTarget::Window { label, x } => (Some(label.clone()), Some(*x)),
        _ => (None, None),
    };
    if let Some(previous) = showing.take().filter(|previous| Some(previous) != label.as_ref()) {
        let _ = app.emit_to(EventTarget::webview_window(previous), "tab-drag-over", None::<f64>);
    }
    if let Some(label) = label {
        let _ = app.emit_to(EventTarget::webview_window(label.clone()), "tab-drag-over", x);
        *showing = Some(label);
    }
}

/// Where a tab dragged from window `source` would go, on the main thread (where AppKit is
/// asked).
async fn drop_target(app: &AppHandle, source: String) -> tauri::Result<DropTarget> {
    let (done, result) = oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = done.send(target_at_cursor(&handle, &source));
    })?;
    result.await.map_err(|_| tauri::Error::FailedToReceiveMessage)
}

fn target_at_cursor(app: &AppHandle, source: &str) -> DropTarget {
    let Some(window) = window_at_cursor(app) else { return DropTarget::Elsewhere };
    if window.label() == source {
        return DropTarget::Here;
    }
    let Some((x, y)) = cursor_in(app, &window) else { return DropTarget::Elsewhere };
    let tab_bar = app.state::<TabDrag>().tab_bars.lock().unwrap().get(window.label()).copied();
    match tab_bar {
        Some(rect) if rect.contains(x, y) => DropTarget::Window { label: window.label().to_owned(), x },
        _ => DropTarget::Elsewhere,
    }
}

/// The cursor in `window`'s page, in CSS pixels. The page fills the window (on macOS the
/// title bar overlays it; on Windows the window has no decorations).
fn cursor_in(app: &AppHandle, window: &WebviewWindow) -> Option<(f64, f64)> {
    let cursor = app.cursor_position().ok()?;
    let origin = window.inner_position().ok()?;
    let scale = window.scale_factor().ok()?;
    Some(((cursor.x - f64::from(origin.x)) / scale, (cursor.y - f64::from(origin.y)) / scale))
}

/// Where a window of `size` opened for a dragged tab goes: with the cursor `grab` from its top
/// left corner, where it was in the window the tab came from, but within the screen's work
/// area (a tab dropped near the bottom of the screen would leave the window mostly below it).
pub fn place_at_cursor(app: &AppHandle, grab: Point, size: tauri::LogicalSize<f64>) -> Option<tauri::LogicalPosition<f64>> {
    let cursor = app.cursor_position().ok()?;
    let monitor = app.available_monitors().ok()?.into_iter().find(|monitor| {
        let (position, size) = (monitor.position(), monitor.size());
        cursor.x >= f64::from(position.x)
            && cursor.y >= f64::from(position.y)
            && cursor.x < f64::from(position.x) + f64::from(size.width)
            && cursor.y < f64::from(position.y) + f64::from(size.height)
    })?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (left, top) = (f64::from(area.position.x) / scale, f64::from(area.position.y) / scale);
    let (right, bottom) = (left + f64::from(area.size.width) / scale, top + f64::from(area.size.height) / scale);
    let x = (cursor.x / scale - grab.x).min(right - size.width).max(left);
    let y = (cursor.y / scale - grab.y).min(bottom - size.height).max(top);
    Some(tauri::LogicalPosition::new(x, y))
}

/// The app's window the system shows on top under the cursor, if any (not when another app's
/// window covers it).
#[cfg(target_os = "macos")]
fn window_at_cursor(app: &AppHandle) -> Option<WebviewWindow> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSWindow};

    let mtm = MainThreadMarker::new()?;
    let number = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(NSEvent::mouseLocation(), 0, mtm);
    app.webview_windows().into_values().find(|window| {
        // SAFETY: on macOS `ns_window` is the window's NSWindow, alive while the window is.
        window.ns_window().is_ok_and(|ns_window| unsafe { &*(ns_window as *const NSWindow) }.windowNumber() == number)
    })
}

#[cfg(windows)]
fn window_at_cursor(app: &AppHandle) -> Option<WebviewWindow> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetAncestor, GetCursorPos, WindowFromPoint, GA_ROOT};

    let mut point = POINT { x: 0, y: 0 };
    // SAFETY: plain calls; `point` is a valid place for the cursor's position. The window at
    // the point can be a child (the maximize button): its top level window is the app's.
    let root = unsafe {
        if GetCursorPos(&mut point) == 0 {
            return None;
        }
        GetAncestor(WindowFromPoint(point), GA_ROOT)
    };
    app.webview_windows().into_values().find(|window| window.hwnd().is_ok_and(|hwnd| hwnd.0 as isize == root as isize))
}

/// Without the system's order of windows: the focused window, then the others in the order
/// they opened, whichever the cursor is over.
#[cfg(not(any(target_os = "macos", windows)))]
fn window_at_cursor(app: &AppHandle) -> Option<WebviewWindow> {
    let mut windows: Vec<WebviewWindow> = app.webview_windows().into_values().collect();
    windows.sort_by_key(|window| (!window.is_focused().unwrap_or(false), crate::window::order(window.label())));
    windows.into_iter().find(|window| {
        let (Some((x, y)), Ok(size), Ok(scale)) = (cursor_in(app, window), window.inner_size(), window.scale_factor()) else {
            return false;
        };
        let size = size.to_logical::<f64>(scale);
        x >= 0.0 && y >= 0.0 && x < size.width && y < size.height
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_contains_its_top_left_but_not_its_far_edges() {
        let rect = Rect { x: 10.0, y: 0.0, width: 100.0, height: 34.0 };
        assert!(rect.contains(10.0, 0.0));
        assert!(rect.contains(109.9, 33.9));
        assert!(!rect.contains(110.0, 10.0));
        assert!(!rect.contains(50.0, 34.0));
        assert!(!rect.contains(9.9, 10.0));
    }
}
