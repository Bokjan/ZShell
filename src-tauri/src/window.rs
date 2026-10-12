//! The app's windows, whose title bar the frontend draws: the tab bar takes its place (see
//! ARCHITECTURE.md). macOS keeps the traffic lights over the web view; on Windows the window
//! has no decorations and the frontend draws the window buttons, helped by the commands here
//! for what a web page can't do natively.
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
use crate::session::SessionManager;

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
fn order(label: &str) -> u32 {
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
    let _ = window.emit("window-closed", window.label());
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        win::forget(hwnd.0 as isize);
    }
    changed(window.app_handle());
}

/// Creates a window from the main window's configuration (which has `create: false`, so the
/// platform's title bar options can be set here rather than in per-platform config files), at
/// `place` (logical position and size) if given.
fn build(
    app: &AppHandle,
    label: &str,
    place: Option<(tauri::LogicalPosition<f64>, tauri::LogicalSize<f64>)>,
) -> tauri::Result<WebviewWindow> {
    let mut config = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned().unwrap_or_default();
    config.label = label.to_owned();
    let builder = WebviewWindowBuilder::from_config(app, &config)?;
    #[cfg(target_os = "macos")]
    let builder =
        builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(TRAFFIC_LIGHTS);
    #[cfg(windows)]
    let builder = builder.decorations(false);
    let builder = match place {
        Some((position, size)) => builder.position(position.x, position.y).inner_size(size.width, size.height),
        None => builder,
    };
    let window = builder.build()?;
    #[cfg(target_os = "macos")]
    refocus_after_full_screen(&window);
    Ok(window)
}

/// Creates the main window.
pub fn create_main(app: &App) -> tauri::Result<()> {
    build(app.handle(), MAIN, None).map(drop)
}

/// A window the size of `source`, a little down and to the right of it.
fn cascade(source: &WebviewWindow) -> tauri::Result<(tauri::LogicalPosition<f64>, tauri::LogicalSize<f64>)> {
    let scale = source.scale_factor()?;
    let position = source.outer_position()?.to_logical::<f64>(scale);
    let size = source.inner_size()?.to_logical::<f64>(scale);
    Ok((tauri::LogicalPosition::new(position.x + CASCADE, position.y + CASCADE), size))
}

/// Opens a window next to this one, which shows `content` first (see [`window_take`]), and
/// returns its label. Async: creating a window from a synchronous command deadlocks on
/// Windows.
#[tauri::command]
pub async fn window_open(
    app: AppHandle,
    window: WebviewWindow,
    windows: State<'_, Windows>,
    content: serde_json::Value,
) -> tauri::Result<String> {
    let label = format!("{LABEL_PREFIX}{}", windows.next.fetch_add(1, Ordering::Relaxed) + 1);
    windows.inbox.lock().unwrap().pending.insert(label.clone(), vec![content]);
    if let Err(e) = build(&app, &label, cascade(&window).ok()) {
        windows.inbox.lock().unwrap().pending.remove(&label);
        return Err(e);
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
    let mut others: Vec<WebviewWindow> =
        app.webview_windows().into_values().filter(|w| w.label() != window.label()).collect();
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
/// `tab-received`, and comes forward. Fails with `window.notFound` if it has closed or is
/// closing.
#[tauri::command]
pub fn window_send_tab(app: AppHandle, windows: State<'_, Windows>, target: String, tab: serde_json::Value) -> Result<()> {
    let window = app.get_webview_window(&target).ok_or_else(|| Error::new("window.notFound"))?;
    {
        let mut inbox = windows.inbox.lock().unwrap();
        if inbox.closing.contains(&target) {
            return Err(Error::new("window.notFound"));
        }
        inbox.pending.entry(target.clone()).or_default().push(serde_json::json!({ "kind": "tab", "tab": tab }));
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
pub async fn app_busy_count(
    app: AppHandle,
    windows: State<'_, Windows>,
    sessions: State<'_, SessionManager>,
) -> tauri::Result<u32> {
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

/// Quits, once the user has confirmed (or nothing was busy).
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}

/// How long after a full screen transition the web view takes the focus back.
#[cfg(target_os = "macos")]
const REFOCUS_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// Once the window has left full screen (its own or the terminals'), the keyboard focus is on
/// the window rather than the web view, so keys go nowhere until it is clicked: the web view
/// gets it back shortly after either transition ends, since focusing it right away is undone
/// by what the window still does as the transition finishes.
#[cfg(target_os = "macos")]
fn refocus_after_full_screen(window: &WebviewWindow) {
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
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            let webview = webview.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(REFOCUS_DELAY).await;
                let _ = webview.set_focus();
            });
        });
        // SAFETY: `ns_window` is the window's NSWindow, which outlives the observer (both last
        // as long as the app). The center keeps the observer, so the token can go.
        let _ = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), Some(&*ns_window.cast::<AnyObject>()), None, &block)
        };
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

/// A double click on the title bar: on macOS what the system setting says ("Double-click a
/// window's title bar to"), elsewhere maximize or restore.
#[tauri::command]
pub fn window_title_double_click(window: WebviewWindow) -> tauri::Result<()> {
    match double_click_action() {
        DoubleClick::Zoom if window.is_maximized()? => window.unmaximize(),
        DoubleClick::Zoom => window.maximize(),
        DoubleClick::Minimize => window.minimize(),
        DoubleClick::Nothing => Ok(()),
    }
}

// Elsewhere than macOS the double click always zooms.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

#[cfg(not(target_os = "macos"))]
fn double_click_action() -> DoubleClick {
    DoubleClick::Zoom
}

/// Shows the window's system menu (Restore, Move, Size, Minimize, Maximize, Close) at the
/// pointer, as a right click on a native title bar does. Windows only.
#[tauri::command]
pub fn window_system_menu(window: WebviewWindow) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd()?.0 as isize;
        window.run_on_main_thread(move || unsafe { win::show_system_menu(hwnd as _) })?;
    }
    #[cfg(not(windows))]
    let _ = window;
    Ok(())
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

/// The frontend's maximize button, in CSS pixels from the top left of the window.
#[derive(Clone, Copy, serde::Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
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

#[cfg(windows)]
mod win {
    //! Windows 11 shows the snap layouts when the pointer rests on whatever answers
    //! `WM_NCHITTEST` with `HTMAXBUTTON`. The web view's own child window covers the whole
    //! client area and gets all mouse messages, and a web page can't give that answer, so a
    //! transparent child window above the web view, over the frontend's maximize button, does.
    //! (Tauri resizes undecorated windows from their top edge the same way.) It never paints,
    //! so the frontend's button shows through; it takes the mouse from the page, so it reports
    //! hover and press for the button's style, and maximizes or restores on click itself.
    //! `WS_EX_LAYERED` or `WS_EX_TRANSPARENT` would make the hit test pass through it.
    //! Each window has its own button, kept by the window's handle.

    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, Once, OnceLock};

    use tauri::{AppHandle, Emitter, EventTarget};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_DEFAULT};
    use windows_sys::Win32::Graphics::Gdi::{GetStockObject, NULL_BRUSH};
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, EnableMenuItem, GetCursorPos, GetParent, GetSystemMenu, IsZoomed, LoadCursorW,
        PostMessageW, RegisterClassW, SetMenuDefaultItem, SetWindowPos, ShowWindow, TrackPopupMenu, HTMAXBUTTON, HWND_TOP,
        IDC_ARROW, MA_NOACTIVATE, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE,
        SC_RESTORE, SC_SIZE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_MOUSEACTIVATE,
        WM_DESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
        WM_NCMOUSEMOVE, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS,
    };

    use super::Rect;

    /// For reporting the button's state to the frontend from the window procedure.
    type IsWindowArranged = unsafe extern "system" fn(HWND) -> BOOL;

    /// `IsWindowArranged` (whether the window is snapped), looked up at run time: older
    /// Windows 10 builds don't have it, and a static import would keep the app from starting.
    fn is_window_arranged() -> Option<IsWindowArranged> {
        static FUNCTION: OnceLock<Option<IsWindowArranged>> = OnceLock::new();
        *FUNCTION.get_or_init(|| unsafe {
            let user32 = GetModuleHandleW(wide("user32.dll").as_ptr());
            if user32.is_null() {
                return None;
            }
            GetProcAddress(user32, c"IsWindowArranged".as_ptr().cast())
                .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, IsWindowArranged>(f))
        })
    }

    /// The border's color while snapped, a `COLORREF`; the system's until the frontend sets it.
    pub(super) static SNAPPED_BORDER: AtomicU32 = AtomicU32::new(DWMWA_COLOR_DEFAULT);

    /// `#rrggbb` as a `COLORREF` (`0x00bbggrr`).
    pub(super) fn colorref(color: &str) -> Option<u32> {
        let hex = color.strip_prefix('#').filter(|hex| hex.len() == 6)?;
        let rgb = u32::from_str_radix(hex, 16).ok()?;
        Some(((rgb & 0xff) << 16) | (rgb & 0xff00) | (rgb >> 16))
    }

    /// The color last set on each window's border, by its handle: moves come often, the
    /// border rarely changes.
    static BORDERS: Mutex<BTreeMap<isize, u32>> = Mutex::new(BTreeMap::new());

    /// # Safety
    /// `hwnd` is one of the app's windows.
    pub(super) unsafe fn update_border(hwnd: HWND) {
        let Some(is_window_arranged) = is_window_arranged() else { return };
        let color =
            if is_window_arranged(hwnd) != 0 { SNAPPED_BORDER.load(Ordering::Relaxed) } else { DWMWA_COLOR_DEFAULT };
        if BORDERS.lock().unwrap().insert(hwnd as isize, color) != Some(color) {
            // Fails on Windows 10, which draws no such border.
            DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR as u32, (&color as *const u32).cast(), size_of::<u32>() as u32);
        }
    }

    pub(super) static APP: OnceLock<AppHandle> = OnceLock::new();

    #[derive(Clone, Copy, Default, PartialEq, serde::Serialize)]
    struct ButtonState {
        hover: bool,
        pressed: bool,
    }

    /// A window's maximize button: its native window, the label of the window it is in, and
    /// the state the frontend was last told.
    struct Button {
        hwnd: HWND,
        label: String,
        state: ButtonState,
    }

    // Only touched on the main thread, which owns the windows and runs their procedures.
    thread_local! {
        /// By the handle of the window each is in.
        static BUTTONS: RefCell<BTreeMap<isize, Button>> = const { RefCell::new(BTreeMap::new()) };
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn state(parent: HWND) -> ButtonState {
        BUTTONS.with_borrow(|buttons| buttons.get(&(parent as isize)).map(|button| button.state).unwrap_or_default())
    }

    /// Records the state of the button in window `parent`, and tells that window's page.
    fn set_state(parent: HWND, state: ButtonState) {
        let label = BUTTONS.with_borrow_mut(|buttons| {
            let button = buttons.get_mut(&(parent as isize))?;
            (button.state != state).then(|| {
                button.state = state;
                button.label.clone()
            })
        });
        if let (Some(label), Some(app)) = (label, APP.get()) {
            let _ = app.emit_to(EventTarget::webview_window(label), "window-maximize-button", state);
        }
    }

    /// Forgets what was kept about a window that is gone, whose handle may be reused.
    pub(super) fn forget(hwnd: isize) {
        BUTTONS.with_borrow_mut(|buttons| buttons.remove(&hwnd));
        BORDERS.lock().unwrap().remove(&hwnd);
    }

    /// # Safety
    /// On the main thread, with `parent` the window labelled `label`.
    pub(super) unsafe fn place_maximize_button(parent: HWND, label: String, rect: Option<Rect>, scale: f64) {
        let existing = BUTTONS.with_borrow(|buttons| buttons.get(&(parent as isize)).map(|button| button.hwnd));
        let button = match existing {
            Some(button) => button,
            None => {
                let button = create_button(parent);
                if button.is_null() {
                    return;
                }
                let entry = Button { hwnd: button, label, state: ButtonState::default() };
                BUTTONS.with_borrow_mut(|buttons| buttons.insert(parent as isize, entry));
                button
            }
        };
        match rect {
            Some(rect) => {
                // Device pixels, rounded the way the page's edges are.
                let left = (rect.x * scale).round() as i32;
                let top = (rect.y * scale).round() as i32;
                let right = ((rect.x + rect.width) * scale).round() as i32;
                let bottom = ((rect.y + rect.height) * scale).round() as i32;
                // On top again each time: the web view and Tauri's resize border are siblings.
                SetWindowPos(button, HWND_TOP, left, top, right - left, bottom - top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
            None => {
                ShowWindow(button, SW_HIDE);
                set_state(parent, ButtonState::default());
            }
        }
    }

    unsafe fn create_button(parent: HWND) -> HWND {
        static REGISTER: Once = Once::new();
        let class = wide("ZShellMaximizeButton");
        let instance = GetModuleHandleW(std::ptr::null());
        REGISTER.call_once(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(button_proc),
                hInstance: instance,
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                hbrBackground: GetStockObject(NULL_BRUSH),
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            RegisterClassW(&class);
        });
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            parent,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    }

    unsafe extern "system" fn button_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_NCHITTEST => HTMAXBUTTON as LRESULT,
            WM_NCMOUSEMOVE => {
                let parent = GetParent(hwnd);
                if !state(parent).hover {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                    set_state(parent, ButtonState { hover: true, ..state(parent) });
                }
                0
            }
            WM_NCMOUSELEAVE => {
                set_state(GetParent(hwnd), ButtonState::default());
                0
            }
            // Not to `DefWindowProcW`, which would run the caption button logic on this child.
            WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
                set_state(GetParent(hwnd), ButtonState { hover: true, pressed: true });
                0
            }
            WM_NCLBUTTONUP => {
                let parent = GetParent(hwnd);
                if state(parent).pressed {
                    set_state(parent, ButtonState { hover: true, pressed: false });
                    let command = if IsZoomed(parent) != 0 { SC_RESTORE } else { SC_MAXIMIZE };
                    PostMessageW(parent, WM_SYSCOMMAND, command as WPARAM, 0);
                }
                0
            }
            // Keyboard focus stays in the web view.
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            // Destroyed with its window, which may be gone before `destroyed` asks its handle.
            WM_DESTROY => {
                let parent = BUTTONS.with_borrow(|buttons| {
                    buttons.iter().find(|(_, button)| button.hwnd == hwnd).map(|(parent, _)| *parent)
                });
                if let Some(parent) = parent {
                    forget(parent);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    /// # Safety
    /// On the main thread, with `hwnd` the main window.
    pub(super) unsafe fn show_system_menu(hwnd: HWND) {
        let menu = GetSystemMenu(hwnd, 0);
        if menu.is_null() {
            return;
        }
        // As the system sets them for a native title bar.
        let maximized = IsZoomed(hwnd) != 0;
        for (item, enabled) in
            [(SC_RESTORE, maximized), (SC_MOVE, !maximized), (SC_SIZE, !maximized), (SC_MINIMIZE, true), (SC_MAXIMIZE, !maximized), (SC_CLOSE, true)]
        {
            EnableMenuItem(menu, item, MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED });
        }
        SetMenuDefaultItem(menu, SC_CLOSE, 0);
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        let command = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, std::ptr::null());
        if command != 0 {
            PostMessageW(hwnd, WM_SYSCOMMAND, command as WPARAM, 0);
        }
    }
}
