//! Dragging a tab out of its window (see ARCHITECTURE.md, "多窗口"): released over another
//! window's tab bar, the tab moves there; released anywhere else outside its window, it opens
//! in a window of its own under the cursor. The page drags with mouse events, as when it
//! reorders tabs, and asks here where the tab would go ([`pointer_moved`]): the window under
//! the cursor is the one the system shows on top there, which only native code can tell, and
//! the cursor is read here in screen coordinates (see [`screen`]), which the page's `screenX`
//! doesn't give reliably across displays with different scales. Wayland gives neither the
//! cursor nor the windows' places, so there the tab leaves its window with the system's drag
//! (`linux.rs`).

#[cfg(target_os = "linux")]
mod linux;

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;
use ts_rs::TS;

use crate::events;

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

/// A point: in a page's CSS pixels, or in screen coordinates (see [`screen`]).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize, TS)]
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
    /// Anywhere else (outside the windows, or over another window but not its tab bar), with
    /// the cursor in screen coordinates, for the window it would open in; `None` where the
    /// system doesn't tell (Wayland), which places that window itself.
    Elsewhere { cursor: Option<Point> },
    /// Out of its window on Wayland: the system's drag has taken over, and where the tab goes
    /// comes when it ends (`tab-drag-native-end`, then [`end`]).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Native,
}

/// The end of a system drag of a tab (`tab-drag-native-end`, to the window it came from).
#[derive(Clone, Serialize, TS)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct TabDragNativeEnd {
    pub id: String,
    pub dropped: bool,
}

/// A tab dropped outside the windows: the cursor in screen coordinates, if known (from
/// [`DropTarget`]), and where it held the tab in the window it came from, in CSS pixels from
/// its top left.
#[derive(Clone, Copy, Deserialize, TS)]
pub struct Dropped {
    pub cursor: Option<Point>,
    pub grab: Point,
}

#[derive(Default)]
pub struct TabDrag {
    /// Where each window's tab bar is (see [`set_tab_bar`]).
    tab_bars: Mutex<HashMap<String, Rect>>,
    drags: Mutex<Drags>,
    /// Where the system drag of a tab went: the drag's id, and the target (`None` when
    /// cancelled). Linux only.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    native_ended: Mutex<Option<(String, Option<DropTarget>)>>,
}

/// The drag going on, if any. Each drag is named by the page; the questions it asks are
/// answered after a while (on the main thread), so one can come back after the drag ended,
/// and must not show where the tab would go any more.
#[derive(Default)]
struct Drags {
    /// The drag going on: its id and the window it drags from.
    current: Option<(String, String)>,
    /// The drag that ended last.
    ended: Option<String>,
    /// The window showing where the tab would go in its tab bar.
    showing: Option<String>,
}

impl Drags {
    /// Shows where `target` would put the tab, in the window it would go into, and stops
    /// showing it in the window that showed it before (the `tab-drag-over` event with the
    /// cursor's `x` in its page, or null).
    fn show(&mut self, app: &AppHandle, target: &DropTarget) {
        let (label, x) = match target {
            DropTarget::Window { label, x } => (Some(label.clone()), Some(*x)),
            _ => (None, None),
        };
        if let Some(previous) = self.showing.take().filter(|previous| Some(previous) != label.as_ref()) {
            events::emit_to(app, previous, events::TabDragOver(None));
        }
        if let Some(label) = label {
            events::emit_to(app, label.clone(), events::TabDragOver(x));
            self.showing = Some(label);
        }
    }

    /// Whether drag `id` from window `source` is the one going on: it starts with its first
    /// question, unless it has ended already.
    fn is_current(&mut self, id: &str, source: &str) -> bool {
        if self.current.is_none() && self.ended.as_deref() != Some(id) {
            self.current = Some((id.to_owned(), source.to_owned()));
        }
        self.current.as_ref().is_some_and(|(current, _)| current == id)
    }

    fn finish(&mut self, id: String) {
        if self.current.as_ref().is_some_and(|(current, _)| *current == id) {
            self.current = None;
        }
        self.ended = Some(id);
    }

    fn end(&mut self, app: &AppHandle, id: String) {
        self.finish(id);
        self.show(app, &DropTarget::Here);
    }
}

/// After a window is gone: a drag from it ends.
pub fn forget(app: &AppHandle, label: &str) {
    let drag = app.state::<TabDrag>();
    drag.tab_bars.lock().unwrap().remove(label);
    let mut drags = drag.drags.lock().unwrap();
    if drags.showing.as_deref() == Some(label) {
        drags.showing = None;
    }
    if let Some((id, _)) = drags.current.clone().filter(|(_, source)| source == label) {
        drags.end(app, id);
    }
}

/// Records where this window's tab bar is, for tabs dragged from other windows.
pub fn set_tab_bar(window: WebviewWindow, drag: &TabDrag, rect: Rect) {
    drag.tab_bars.lock().unwrap().insert(window.label().to_owned(), rect);
}

/// Where the tab of drag `id` from this window would go if released now; the window it would
/// go into shows where, while the drag goes on. `outside`: the page saw the pointer outside
/// it (see [`target_at_cursor`]).
pub async fn pointer_moved(app: AppHandle, window: WebviewWindow, id: String, outside: bool) -> tauri::Result<DropTarget> {
    let source = window.label().to_owned();
    let target = drop_target(&app, source.clone(), outside, Some(id.clone())).await?;
    let drag = app.state::<TabDrag>();
    let mut drags = drag.drags.lock().unwrap();
    if drags.is_current(&id, &source) {
        drags.show(&app, &target);
    }
    Ok(target)
}

/// Ends drag `id` from this window: where the tab goes if `dropped` (rather than cancelled).
pub async fn end(
    app: AppHandle,
    window: WebviewWindow,
    drag: &TabDrag,
    id: String,
    dropped: bool,
    outside: bool,
) -> tauri::Result<Option<DropTarget>> {
    drag.drags.lock().unwrap().end(&app, id.clone());
    let native = drag.native_ended.lock().unwrap().take_if(|(ended, _)| *ended == id);
    if let Some((_, target)) = native {
        return Ok(target);
    }
    if !dropped {
        return Ok(None);
    }
    drop_target(&app, window.label().to_owned(), outside, None).await.map(Some)
}

/// Where a tab dragged from window `source` would go, on the main thread (where AppKit is
/// asked).
async fn drop_target(app: &AppHandle, source: String, outside: bool, id: Option<String>) -> tauri::Result<DropTarget> {
    let (done, result) = oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = done.send(target_at_cursor(&handle, &source, outside, id));
    })?;
    result.await.map_err(|_| tauri::Error::FailedToReceiveMessage)
}

/// Where a tab dragged from window `source` would go, from the cursor and the windows under
/// it. Wayland tells an app neither where the cursor is on the screen nor where its windows
/// are: there, once the page saw the cursor `outside` it during drag `id`, the system's drag
/// takes over (`linux.rs`), or failing that the tab goes into a window of its own, which the
/// compositor places.
fn target_at_cursor(app: &AppHandle, source: &str, outside: bool, id: Option<String>) -> DropTarget {
    #[cfg(target_os = "linux")]
    if gtk::gdk::Display::default().is_some_and(|display| gtk::gdk::prelude::DisplayExtManual::backend(&display).is_wayland()) {
        return match (outside, id) {
            (false, _) => DropTarget::Here,
            (true, Some(id)) if linux::start(app, source, id.clone()) => DropTarget::Native,
            (true, _) => DropTarget::Elsewhere { cursor: None },
        };
    }
    let _ = (outside, id);
    // Nowhere new if the cursor can't be read.
    let Some(cursor) = cursor(app) else { return DropTarget::Here };
    let Some(window) = window_at_cursor(app, cursor) else { return DropTarget::Elsewhere { cursor: Some(cursor) } };
    if window.label() == source {
        return DropTarget::Here;
    }
    let tab_bar = app.state::<TabDrag>().tab_bars.lock().unwrap().get(window.label()).copied();
    match (tab_bar, page_point(&window, cursor)) {
        (Some(rect), Some(Point { x, y })) if rect.contains(x, y) => DropTarget::Window { label: window.label().to_owned(), x },
        _ => DropTarget::Elsewhere { cursor: Some(cursor) },
    }
}

/// A position in screen coordinates: ones that positions on displays with different scales
/// can be compared in. On macOS these are the global points AppKit works in, which are also
/// the page's CSS pixels: tao gives each position in pixels of a scale of its own (the cursor
/// in the main display's, a window's origin in its display's, a display's bounds in its own),
/// and dividing by that scale gives the points back. Elsewhere physical pixels are global, and
/// logical ones aren't (a logical position means a different place on each display).
#[cfg(target_os = "macos")]
pub fn screen(physical: f64, scale: f64) -> f64 {
    physical / scale
}

#[cfg(not(target_os = "macos"))]
pub fn screen(physical: f64, _scale: f64) -> f64 {
    physical
}

/// How many screen units a CSS pixel is, on a display of `scale`.
#[cfg(target_os = "macos")]
pub fn screen_per_css(_scale: f64) -> f64 {
    1.0
}

#[cfg(not(target_os = "macos"))]
pub fn screen_per_css(scale: f64) -> f64 {
    scale
}

/// A position in screen coordinates, as Tauri takes it.
#[cfg(target_os = "macos")]
pub fn screen_position(x: f64, y: f64) -> tauri::Position {
    tauri::LogicalPosition::new(x, y).into()
}

#[cfg(not(target_os = "macos"))]
pub fn screen_position(x: f64, y: f64) -> tauri::Position {
    tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32).into()
}

/// The cursor in screen coordinates.
fn cursor(app: &AppHandle) -> Option<Point> {
    let cursor = app.cursor_position().ok()?;
    // On macOS in the main display's scale.
    let scale =
        if cfg!(target_os = "macos") { app.primary_monitor().ok().flatten().map_or(1.0, |monitor| monitor.scale_factor()) } else { 1.0 };
    Some(Point { x: screen(cursor.x, scale), y: screen(cursor.y, scale) })
}

/// Where `point` (screen coordinates) is in `window`'s page, in CSS pixels. The page fills the
/// window (on macOS the title bar overlays it; on Windows the window has no decorations).
fn page_point(window: &WebviewWindow, point: Point) -> Option<Point> {
    let origin = window.inner_position().ok()?;
    let scale = window.scale_factor().ok()?;
    let per_css = screen_per_css(scale);
    Some(Point { x: (point.x - screen(f64::from(origin.x), scale)) / per_css, y: (point.y - screen(f64::from(origin.y), scale)) / per_css })
}

/// Where a window the size of `source` opened for a tab dropped outside the windows goes:
/// holding the tab where it was in `source`, but within the work area of the display the
/// cursor is on (a tab dropped near the bottom of the screen would leave the window mostly
/// below it).
pub fn place_dropped(app: &AppHandle, source: &WebviewWindow, dropped: Dropped) -> Option<tauri::Position> {
    let size = source.inner_size().ok()?.to_logical::<f64>(source.scale_factor().ok()?);
    let Dropped { cursor, grab } = dropped;
    let cursor = cursor?;
    let monitor = app.available_monitors().ok()?.into_iter().find(|monitor| {
        let scale = monitor.scale_factor();
        let (position, size) = (monitor.position(), monitor.size());
        let (left, top) = (screen(f64::from(position.x), scale), screen(f64::from(position.y), scale));
        let (width, height) = (screen(f64::from(size.width), scale), screen(f64::from(size.height), scale));
        cursor.x >= left && cursor.y >= top && cursor.x < left + width && cursor.y < top + height
    })?;
    let scale = monitor.scale_factor();
    let per_css = screen_per_css(scale);
    let area = monitor.work_area();
    let (left, top) = (screen(f64::from(area.position.x), scale), screen(f64::from(area.position.y), scale));
    let right = left + screen(f64::from(area.size.width), scale);
    let bottom = top + screen(f64::from(area.size.height), scale);
    let x = (cursor.x - grab.x * per_css).min(right - size.width * per_css).max(left);
    let y = (cursor.y - grab.y * per_css).min(bottom - size.height * per_css).max(top);
    Some(screen_position(x, y))
}

/// The app's window the system shows on top under the cursor, if any (not when another app's
/// window covers it).
#[cfg(target_os = "macos")]
fn window_at_cursor(app: &AppHandle, _cursor: Point) -> Option<WebviewWindow> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSWindow};

    let mtm = MainThreadMarker::new()?;
    let number = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(NSEvent::mouseLocation(), 0, mtm);
    crate::window::app_windows(app).into_iter().find(|window| {
        // SAFETY: on macOS `ns_window` is the window's NSWindow, alive while the window is.
        window.ns_window().is_ok_and(|ns_window| unsafe { &*(ns_window as *const NSWindow) }.windowNumber() == number)
    })
}

#[cfg(windows)]
fn window_at_cursor(app: &AppHandle, _cursor: Point) -> Option<WebviewWindow> {
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
    crate::window::app_windows(app).into_iter().find(|window| window.hwnd().is_ok_and(|hwnd| hwnd.0 as isize == root as isize))
}

/// Without the system's order of windows: the focused window, then the others in the order
/// they opened, whichever the cursor is over.
#[cfg(not(any(target_os = "macos", windows)))]
fn window_at_cursor(app: &AppHandle, cursor: Point) -> Option<WebviewWindow> {
    let mut windows: Vec<WebviewWindow> = crate::window::app_windows(app);
    windows.sort_by_key(|window| (!window.is_focused().unwrap_or(false), crate::window::order(window.label())));
    windows.into_iter().find(|window| {
        let (Some(Point { x, y }), Ok(size), Ok(scale)) = (page_point(window, cursor), window.inner_size(), window.scale_factor()) else {
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

    /// An answer that comes back after its drag ended doesn't count, nor starts it again; the
    /// next drag does.
    #[test]
    fn a_drag_that_ended_stays_ended() {
        let mut drags = Drags::default();
        assert!(drags.is_current("a", "main"));
        assert!(!drags.is_current("b", "window-1"));
        drags.finish("a".into());
        assert!(!drags.is_current("a", "main"));
        assert!(drags.current.is_none());
        assert!(drags.is_current("b", "window-1"));
        // Ended before its first answer came back.
        drags.finish("c".into());
        assert!(drags.is_current("b", "window-1"));
    }
}
