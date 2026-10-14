//! The app's windows, whose title bar the frontend draws: the tab bar takes its place (see
//! ARCHITECTURE.md). macOS keeps the traffic lights over the web view; on Windows the window
//! has no decorations and the frontend draws the window buttons, helped by the commands here
//! for what a web page can't do natively (`win.rs`); on Linux GTK draws the window's frame
//! around an empty title bar and the frontend draws the buttons (`linux.rs`).
//!
//! The first window is `main`; the others open for a tab moved out of a window or a session
//! opened in a new window, and are labelled `window-1`, `window-2` and so on (the capability
//! grants `window-*`). What a window is to show is kept for its page to take ([`window_take`]):
//! what it opened for (a tab, or a session to open), then tabs other windows give it, which
//! only the frontend reads. Kept rather than sent, so that a page still loading gets them, and
//! a window closing once its last tab is gone never takes one with it ([`window_close_empty`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{App, AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::session::{SessionId, SessionManager};
use crate::tab_drag::{self, Dropped};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{watch_buttons, watch_theme, web_view};
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::keep_behind;

const MAIN: &str = "main";
const LABEL_PREFIX: &str = "window-";

/// How far a new window is from the one it came from, down and to the right.
const CASCADE: f64 = 28.0;

/// How long quitting waits for the windows to say how many of their tabs are busy.
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the traffic lights sit, centered in the frontend's 34px title bar.
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHTS: tauri::LogicalPosition<f64> = tauri::LogicalPosition { x: 14.0, y: 19.0 };

/// The app's windows beyond what Tauri keeps.
#[derive(Default)]
pub struct Windows {
    next: AtomicU32,
    inbox: Mutex<Inbox>,
    /// The window that last had the focus, which menu items act on.
    focused: Mutex<String>,
    next_query: AtomicU64,
    busy: Mutex<HashMap<u64, BusyQuery>>,
}

/// What the windows are to show, until their pages take it.
#[derive(Default)]
struct Inbox {
    pending: HashMap<String, Vec<serde_json::Value>>,
    /// Windows closing once their last tab has gone, which take nothing more.
    closing: HashSet<String>,
}

/// Asking every window how many of its tabs closing would end (see [`app_busy_count`]).
struct BusyQuery {
    waiting: HashSet<String>,
    count: u32,
    done: Option<oneshot::Sender<()>>,
}

/// Where a window is in the order they opened: `main` first.
pub(crate) fn order(label: &str) -> u32 {
    label.strip_prefix(LABEL_PREFIX).and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// The window menu items act on, and that comes forward when the app is opened again: the
/// one with the focus, else the one that last had it, else the first one still open. Asked
/// rather than only recorded: a new window gets the focus as it is created, before its
/// events are delivered, so its first `Focused` event never comes.
pub fn current(app: &AppHandle) -> Option<WebviewWindow> {
    let windows = app.webview_windows();
    if let Some(window) = windows.values().find(|w| w.is_focused().unwrap_or(false)) {
        return Some(window.clone());
    }
    let label = app.state::<Windows>().focused.lock().unwrap().clone();
    windows.get(&label).or_else(|| windows.values().min_by_key(|w| order(w.label()))).cloned()
}

/// Tells every window that windows opened, closed or changed their titles (`windows-changed`).
fn changed(app: &AppHandle) {
    let _ = app.emit("windows-changed", ());
}

/// Records the window that has the focus now.
pub fn focused(window: &tauri::Window) {
    *window.state::<Windows>().focused.lock().unwrap() = window.label().to_owned();
}

/// After a window is gone: what was kept about it goes, and the other windows hear
/// (`window-closed` with its label, besides `windows-changed`).
pub fn destroyed(window: &tauri::Window) {
    {
        let windows = window.state::<Windows>();
        let mut inbox = windows.inbox.lock().unwrap();
        inbox.pending.remove(window.label());
        inbox.closing.remove(window.label());
    }
    tab_drag::forget(window.app_handle(), window.label());
    let _ = window.emit("window-closed", window.label());
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        win::forget(hwnd.0 as isize);
    }
    changed(window.app_handle());
}

/// Where a new window goes: its top left corner in screen coordinates (see
/// [`tab_drag::screen`]), and its size.
type Place = (tauri::Position, tauri::LogicalSize<f64>);

/// Creates a window from the main window's configuration (which has `create: false`, so the
/// platform's title bar options can be set here rather than in per-platform config files), at
/// `place` if given.
fn build(app: &AppHandle, label: &str, place: Option<Place>) -> tauri::Result<WebviewWindow> {
    let mut config = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned().unwrap_or_default();
    config.label = label.to_owned();
    // WebView2 would otherwise offer values typed into earlier forms, and doesn't always honor
    // `autocomplete="off"`. A no-op on the other platforms, whose web views have no such list.
    let builder = WebviewWindowBuilder::from_config(app, &config)?.general_autofill_enabled(false);
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(TRAFFIC_LIGHTS);
    #[cfg(windows)]
    let builder = builder.decorations(false);
    // Placed once created, then shown: the builder takes a logical position, which on Windows
    // means a different place on each display. On Linux the title bar is replaced before the
    // window is shown.
    let hidden = place.is_some() || cfg!(target_os = "linux");
    let builder = if hidden { builder.visible(false) } else { builder };
    let window = builder.build()?;
    #[cfg(target_os = "linux")]
    {
        let window = window.clone();
        app.run_on_main_thread(move || {
            let _ = linux::prepare(&window);
        })?;
    }
    if let Some((position, size)) = place {
        window.set_position(position)?;
        // After moving: on another display the size is in that display's scale.
        window.set_size(size)?;
    }
    if hidden {
        window.show()?;
    }
    #[cfg(target_os = "macos")]
    watch_full_screen(&window);
    Ok(window)
}

/// Creates the main window.
pub fn create_main(app: &App) -> tauri::Result<()> {
    #[cfg(windows)]
    win::watch_foreground();
    build(app.handle(), MAIN, None).map(drop)
}

fn size(window: &WebviewWindow) -> tauri::Result<tauri::LogicalSize<f64>> {
    Ok(window.inner_size()?.to_logical::<f64>(window.scale_factor()?))
}

/// A window the size of `source`, a little down and to the right of it.
fn cascade(source: &WebviewWindow) -> tauri::Result<Place> {
    let position = source.outer_position()?;
    let scale = source.scale_factor()?;
    let offset = CASCADE * tab_drag::screen_per_css(scale);
    let x = tab_drag::screen(f64::from(position.x), scale) + offset;
    let y = tab_drag::screen(f64::from(position.y), scale) + offset;
    Ok((tab_drag::screen_position(x, y), size(source)?))
}

/// Opens a window the size of this one, which shows `content` first (see [`window_take`]),
/// and returns its label. It goes next to this one, or where a tab was `dropped` outside the
/// windows (see [`tab_drag::place_dropped`]). `sessions`: this window's sessions that a tab in
/// `content` shows, which the new window may take (see [`SessionManager::offer`]). Async:
/// creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn window_open(
    app: AppHandle,
    window: WebviewWindow,
    windows: State<'_, Windows>,
    session_manager: State<'_, SessionManager>,
    content: serde_json::Value,
    sessions: Vec<SessionId>,
    dropped: Option<Dropped>,
) -> Result<String> {
    let label = format!("{LABEL_PREFIX}{}", windows.next.fetch_add(1, Ordering::Relaxed) + 1);
    session_manager.offer(window.label(), &sessions, &label)?;
    windows.inbox.lock().unwrap().pending.insert(label.clone(), vec![content]);
    let at_drop = || Some((tab_drag::place_dropped(&app, &window, dropped?)?, size(&window).ok()?));
    let place = at_drop().or_else(|| cascade(&window).ok());
    if let Err(e) = build(&app, &label, place) {
        windows.inbox.lock().unwrap().pending.remove(&label);
        return Err(e.into());
    }
    changed(&app);
    Ok(label)
}

/// What this window is to show and hasn't taken yet: what it opened for, and tabs given to it
/// ([`window_send_tab`]), in order. Each is taken once.
#[tauri::command]
pub fn window_take(window: WebviewWindow, windows: State<'_, Windows>) -> Vec<serde_json::Value> {
    windows.inbox.lock().unwrap().pending.remove(window.label()).unwrap_or_default()
}

/// Closes this window, which has no tabs left, unless it is the last one or has been given a
/// tab it hasn't taken yet; returns whether it closes. From then on it takes no tabs.
#[tauri::command]
pub fn window_close_empty(app: AppHandle, window: WebviewWindow, windows: State<'_, Windows>) -> tauri::Result<bool> {
    let mut inbox = windows.inbox.lock().unwrap();
    let label = window.label();
    let given = inbox.pending.get(label).is_some_and(|pending| !pending.is_empty());
    if given || app.webview_windows().len() < 2 {
        return Ok(false);
    }
    inbox.closing.insert(label.to_owned());
    drop(inbox);
    window.destroy()?;
    Ok(true)
}

/// Another window, as the tab menu lists it.
#[derive(Serialize, TS)]
pub struct WindowInfo {
    pub label: String,
    pub title: String,
}

/// The other windows, in the order they opened.
#[tauri::command]
pub fn window_list(app: AppHandle, window: WebviewWindow) -> Vec<WindowInfo> {
    let mut others: Vec<WebviewWindow> = app.webview_windows().into_values().filter(|w| w.label() != window.label()).collect();
    others.sort_by_key(|w| order(w.label()));
    others.into_iter().map(|w| WindowInfo { title: w.title().unwrap_or_default(), label: w.label().to_owned() }).collect()
}

/// Names the window after its tab (the Window menu, the taskbar, and the tab menus of the
/// other windows show it).
#[tauri::command]
pub fn window_set_title(app: AppHandle, window: WebviewWindow, title: String) -> tauri::Result<()> {
    if window.title()? != title {
        window.set_title(&title)?;
        changed(&app);
    }
    Ok(())
}

/// Gives a tab to window `target`, which takes it (see [`window_take`]) when it hears
/// `tab-received`, and comes forward. `at`: where in its page it was dropped (CSS pixels from
/// the left), to put it there among the tabs. `sessions`: this window's sessions that the tab
/// shows, which `target` may take (see [`SessionManager::offer`]). Fails with
/// `window.notFound` if it has closed or is closing.
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
    let source = window.label().to_owned();
    let window = app.get_webview_window(&target).ok_or_else(|| Error::new("window.notFound"))?;
    {
        let mut inbox = windows.inbox.lock().unwrap();
        if inbox.closing.contains(&target) {
            return Err(Error::new("window.notFound"));
        }
        session_manager.offer(&source, &sessions, &target)?;
        inbox.pending.entry(target.clone()).or_default().push(serde_json::json!({ "kind": "tab", "tab": tab, "at": at }));
    }
    let _ = app.emit_to(EventTarget::webview_window(target), "tab-received", ());
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

/// Tells window `target`, which gave this one a tab, that its pane `pane` couldn't move here
/// (its session is still there): the `pane-stayed` event.
#[tauri::command]
pub fn window_pane_stayed(app: AppHandle, target: String, pane: u32) {
    let _ = app.emit_to(EventTarget::webview_window(target), "pane-stayed", pane);
}

/// How many tabs quitting would end, in all the windows: each is asked (the `busy-query`
/// event, answered with [`window_busy_reply`]), since only its page knows which of its tabs
/// are busy. A window that doesn't answer in time (its page still loading) counts all the
/// sessions it shows.
#[tauri::command]
pub async fn app_busy_count(app: AppHandle, windows: State<'_, Windows>, sessions: State<'_, SessionManager>) -> tauri::Result<u32> {
    let labels: HashSet<String> = app.webview_windows().into_keys().collect();
    let id = windows.next_query.fetch_add(1, Ordering::Relaxed);
    let (done, answered) = oneshot::channel();
    windows.busy.lock().unwrap().insert(id, BusyQuery { waiting: labels.clone(), count: 0, done: Some(done) });
    for label in labels {
        let _ = app.emit_to(EventTarget::webview_window(label), "busy-query", id);
    }
    let _ = tokio::time::timeout(BUSY_TIMEOUT, answered).await;
    let Some(query) = windows.busy.lock().unwrap().remove(&id) else { return Ok(0) };
    Ok(query.count + query.waiting.iter().map(|label| sessions.owned(label)).sum::<u32>())
}

/// A window's answer to `busy-query`.
#[tauri::command]
pub fn window_busy_reply(window: WebviewWindow, windows: State<'_, Windows>, query: u64, count: u32) {
    let mut queries = windows.busy.lock().unwrap();
    let Some(query) = queries.get_mut(&query) else { return };
    if query.waiting.remove(window.label()) {
        query.count += count;
    }
    if query.waiting.is_empty() {
        if let Some(done) = query.done.take() {
            let _ = done.send(());
        }
    }
}

/// Plays the system's alert sound, for a terminal's bell (the bell sound setting).
#[tauri::command]
pub fn app_bell(app: AppHandle) {
    let _ = app.run_on_main_thread(|| {
        #[cfg(target_os = "macos")]
        objc2_app_kit::NSBeep();
        // SAFETY: takes no pointers; `MB_OK` is the default sound.
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(windows_sys::Win32::UI::WindowsAndMessaging::MB_OK);
        }
        #[cfg(target_os = "linux")]
        if let Some(display) = gtk::gdk::Display::default() {
            display.beep();
        }
    });
}

/// Quits, once the user has confirmed (or nothing was busy).
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}

/// How long after a full screen transition the web view takes the focus back.
#[cfg(target_os = "macos")]
const REFOCUS_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// As the window enters or leaves full screen (its own or the terminals'): the menu bar's full
/// screen item says what it now does (`menu::show_full_screen`), and the web view gets the
/// keyboard focus back. Once the window has left full screen, the focus is on the window
/// rather than the web view, so keys go nowhere until it is clicked: the web view gets it
/// back shortly after either transition ends, since focusing it right away is undone by what
/// the window still does as the transition finishes.
#[cfg(target_os = "macos")]
fn watch_full_screen(window: &WebviewWindow) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification};
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let Ok(ns_window) = window.ns_window() else { return };
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the notification names are AppKit's constants.
    for name in unsafe { [NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification] } {
        let webview = window.as_ref().clone();
        let app = window.app_handle().clone();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            crate::menu::show_full_screen(&app);
            let webview = webview.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(REFOCUS_DELAY).await;
                let _ = webview.set_focus();
            });
        });
        // SAFETY: `ns_window` is the window's NSWindow, which outlives the observer (both last
        // as long as the app). The center keeps the observer, so the token can go.
        let _ =
            unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), Some(&*ns_window.cast::<AnyObject>()), None, &block) };
    }
}

/// Brings the current window (see [`current`]) forward, restored if it was minimized.
#[cfg(windows)]
pub fn show_current(app: &AppHandle) {
    if let Some(window) = current(app) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Brings the current window (see [`current`]) forward for an instance started later, with
/// the activation token it was started with (see `instance::listen`). Linux only.
#[cfg(target_os = "linux")]
pub fn come_forward(app: &AppHandle, token: String) {
    let app_ = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = current(&app_) {
            let _ = window.unminimize();
            let _ = window.show();
            linux::present(&window, &token);
        }
    });
}

/// A double click on the title bar: on macOS and Linux what the system setting says
/// ("Double-click a window's title bar to"), on Windows maximize or restore.
#[tauri::command]
pub fn window_title_double_click(window: WebviewWindow) -> tauri::Result<()> {
    match double_click_action() {
        DoubleClick::Zoom if window.is_maximized()? => window.unmaximize(),
        DoubleClick::Zoom => window.maximize(),
        DoubleClick::Minimize => window.minimize(),
        DoubleClick::Nothing => Ok(()),
    }
}

// On Windows the double click always zooms.
#[cfg_attr(windows, allow(dead_code))]
enum DoubleClick {
    Zoom,
    Minimize,
    Nothing,
}

#[cfg(target_os = "macos")]
fn double_click_action() -> DoubleClick {
    use objc2_foundation::{NSString, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    // "Maximize" (Zoom), "Fill" (macOS 15; zooming fills the screen too), "Minimize" or "None";
    // older systems only had the minimize switch.
    match defaults.stringForKey(&NSString::from_str("AppleActionOnDoubleClick")).map(|v| v.to_string()).as_deref() {
        Some("Minimize") => DoubleClick::Minimize,
        Some("None") => DoubleClick::Nothing,
        Some(_) => DoubleClick::Zoom,
        None if defaults.boolForKey(&NSString::from_str("AppleMiniaturizeOnDoubleClick")) => DoubleClick::Minimize,
        None => DoubleClick::Zoom,
    }
}

#[cfg(target_os = "linux")]
use linux::double_click_action;

#[cfg(windows)]
fn double_click_action() -> DoubleClick {
    DoubleClick::Zoom
}

/// Shows the window's system menu (Restore, Move, Size, Minimize, Maximize, Close) at the
/// pointer, as a right click on a native title bar does. Windows and Linux.
#[tauri::command]
pub fn window_system_menu(window: WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd()?.0 as isize;
        window.run_on_main_thread(move || unsafe { win::show_system_menu(hwnd as _) })?;
    }
    #[cfg(target_os = "linux")]
    window.clone().run_on_main_thread(move || linux::show_window_menu(&window))?;
    #[cfg(target_os = "macos")]
    let _ = window;
    Ok(())
}

/// A file dialog of the dialog plugin is opening (`open`) or has closed, for showing the
/// pointer in it on Windows (see `win::file_dialog`).
#[tauri::command]
pub fn window_file_dialog(window: WebviewWindow, open: bool) -> tauri::Result<()> {
    #[cfg(windows)]
    window.run_on_main_thread(move || win::file_dialog(open))?;
    #[cfg(not(windows))]
    let _ = (window, open);
    Ok(())
}

/// Where files can be dragged out of the window (the SFTP file list); `None` when there is
/// none. A press there while the window is in the background doesn't bring the window
/// forward (the frontend does once it is released without dragging; see `macos.rs`). macOS
/// only: on Windows the web view always brings it forward, and a drag puts it back
/// ([`keep_behind`]).
#[tauri::command]
pub fn window_set_drag_area(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    window.with_webview(move |webview| {
        // SAFETY: on macOS the platform webview is a WKWebView, an NSView.
        let view: &objc2_app_kit::NSView = unsafe { &*(webview.inner() as *const objc2_app_kit::NSView) };
        macos::set_area(view, rect);
    })?;
    #[cfg(not(target_os = "macos"))]
    let _ = (window, rect);
    Ok(())
}

/// A window button the frontend draws.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// The window buttons the frontend draws, in order: none on macOS (the traffic lights), all
/// three on Windows, and on Linux those the desktop's layout has, which may change
/// (`window-buttons-changed`).
#[tauri::command]
pub fn window_buttons() -> Vec<WindowButton> {
    #[cfg(target_os = "linux")]
    return linux::buttons();
    #[cfg(windows)]
    return vec![WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close];
    #[cfg(target_os = "macos")]
    return Vec::new();
}

/// Sets the window's appearance (all windows' on Linux, where it is GTK's); `None` follows
/// the system's. Off the main thread: on Linux it may ask the settings portal.
#[tauri::command]
pub async fn window_set_theme(window: WebviewWindow, theme: Option<tauri::Theme>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    return linux::set_theme(&window, theme);
    #[cfg(not(target_os = "linux"))]
    window.set_theme(theme)
}

/// The title bar's color (`#rrggbb`), for the window's border while it is snapped (see
/// [`update_border`]); set with the theme. Windows only.
#[tauri::command]
pub fn window_set_snapped_border_color(window: WebviewWindow, color: String) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        if let Some(color) = win::colorref(&color) {
            win::SNAPPED_BORDER.store(color, std::sync::atomic::Ordering::Relaxed);
            let hwnd = window.hwnd()?.0 as isize;
            unsafe { win::update_border(hwnd as _) };
        }
    }
    #[cfg(not(windows))]
    let _ = (window, color);
    Ok(())
}

/// A part of the page (the maximize button, the drag area), in CSS pixels from the top left
/// of the window.
#[derive(Clone, Copy, serde::Deserialize)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Places the native window that makes the frontend's maximize button behave like a native
/// one (the Windows 11 snap layouts), over `rect`; `None` hides it. Windows only.
#[tauri::command]
pub fn window_set_maximize_button(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        win::APP.get_or_init(|| window.app_handle().clone());
        let parent = window.hwnd()?.0 as isize;
        let scale = window.scale_factor()?;
        let label = window.label().to_owned();
        window.run_on_main_thread(move || unsafe { win::place_maximize_button(parent as _, label, rect, scale) })?;
    }
    #[cfg(not(windows))]
    let _ = (window, rect);
    Ok(())
}

/// After the window moved or was resized: Windows 11 frames a window that isn't maximized
/// with a 1px border outside the web view, which stays when the window is snapped to part of
/// the screen, where it looks like a gap along the screen's edges. The space can't be given
/// to the web view without taking over the frame's layout from Tao, so while snapped the
/// border takes the title bar's color and blends in. Windows only.
pub fn update_border(window: &tauri::Window) {
    #[cfg(windows)]
    {
        if let Ok(hwnd) = window.hwnd() {
            unsafe { win::update_border(hwnd.0) };
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}
