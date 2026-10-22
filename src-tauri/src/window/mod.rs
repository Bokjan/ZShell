//! The app's windows, whose title bar the frontend draws: the tab bar takes its place (see
//! ARCHITECTURE.md). macOS keeps the traffic lights over the web view; on Windows the window
//! has no decorations and the frontend draws the window buttons, helped by the commands here
//! for what a web page can't do natively (`win.rs`); on Linux GTK draws the window's frame
//! around an empty title bar and the frontend draws the buttons (`linux.rs`).
//!
//! The first window is `main`; the others open for a tab moved out of a window or a session
//! opened in a new window, and are labelled `window-1`, `window-2` and so on (the capability
//! grants `window-*`). What a window is to show is kept for its page to take ([`take`]):
//! what it opened for (a tab, or a session to open), then tabs other windows give it, which
//! only the frontend reads. Kept rather than sent, so that a page still loading gets them, and
//! a window closing once its last tab is gone never takes one with it ([`close_empty`]).
//!
//! What each platform does to its windows is in its own file (`macos.rs`, `win.rs`,
//! `linux.rs`), which this one reaches as `platform`. Each provides the same functions, doing
//! nothing itself where the platform has nothing to do, so that one missing a function doesn't
//! compile. Those that need the main thread (where AppKit, Win32 and GTK calls are made) get
//! there by themselves or are called there ([`on_main_thread`]), rather than relying on the
//! command that calls them being synchronous.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{App, AppHandle, Manager, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::events;
use crate::keep_awake::KeepAwake;
use crate::session::{SessionId, SessionManager};
use crate::settings::SettingsStore;
use crate::tab_drag::{self, Dropped};
use inbox::{BusyQueries, Inbox};

mod inbox;
#[cfg(target_os = "linux")]
mod linux;
pub mod progress;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "linux")]
pub use linux::{watch_buttons, watch_theme, web_view};
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as platform;
#[cfg(windows)]
pub use win::{keep_behind, need_pointer};

const MAIN: &str = "main";
const LABEL_PREFIX: &str = "window-";

/// How far a new window is from the one it came from, down and to the right.
const CASCADE: f64 = 28.0;

/// How long quitting waits for the windows to say how many of their tabs are busy.
const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// The app's windows beyond what Tauri keeps.
#[derive(Default)]
pub struct Windows {
    next: AtomicU32,
    inbox: Mutex<Inbox>,
    /// The window that last had the focus, which menu items act on.
    focused: Mutex<String>,
    busy: Mutex<BusyQueries>,
    progress: Mutex<progress::Progresses>,
}

/// What a window is to show: a tab moved out of another window (dropped `at` that many CSS
/// pixels from the left of the page, if dragged there), or a saved session to open. The tab
/// is the frontend's (its panes, their layout and state), passed on without being read.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WindowContent {
    Tab {
        #[ts(type = "unknown")]
        tab: serde_json::Value,
        at: Option<f64>,
    },
    Profile {
        profile_id: String,
        title: String,
    },
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
    events::emit(app, events::WindowsChanged);
}

/// Records the window that has the focus now.
pub fn focused(window: &tauri::Window) {
    *window.state::<Windows>().focused.lock().unwrap() = window.label().to_owned();
}

/// After a window is gone: what was kept about it goes, and the other windows hear
/// (`window-closed` with its label, besides `windows-changed`).
pub fn destroyed(window: &tauri::Window) {
    let windows = window.state::<Windows>();
    windows.inbox.lock().unwrap().forget(window.label());
    // Its sessions closed with it.
    windows.busy.lock().unwrap().gone(window.label());
    tab_drag::forget(window.app_handle(), window.label());
    progress::forget(window.app_handle(), window.label());
    window.state::<Arc<KeepAwake>>().set_connected(window.label(), false);
    events::emit(window, events::WindowClosed(window.label().to_owned()));
    platform::forget(window);
    changed(window.app_handle());
}

/// Window `label`'s page loads again (see `page_gone`): the tabs its page had taken went with
/// it, before their sessions moved there perhaps. Those stay with the windows that offered them,
/// which hear (`offers-withdrawn`); the new page takes only what it hasn't yet.
pub fn page_loading(app: &AppHandle, label: &str) {
    progress::forget(app, label);
    app.state::<Arc<KeepAwake>>().set_connected(label, false);
    let windows = app.state::<Windows>();
    let withdrawn = {
        // Held across, as when a tab is given: no offer for a tab not yet taken is lost.
        let inbox = windows.inbox.lock().unwrap();
        app.state::<SessionManager>().withdraw_offers(label, &inbox.untaken(label))
    };
    for (webview, sessions) in withdrawn {
        events::emit_to(app, webview, events::OffersWithdrawn(sessions));
    }
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
    let builder = platform::title_bar(WebviewWindowBuilder::from_config(app, &config)?.general_autofill_enabled(false));
    let builder = platform::background(builder, app.state::<SettingsStore>().get().appearance);
    // Placed once created, then shown: the builder takes a logical position, which on Windows
    // means a different place on each display. Some platforms prepare the window before it is
    // first shown too.
    let hidden = place.is_some() || platform::PREPARED_HIDDEN;
    let builder = if hidden { builder.visible(false) } else { builder };
    let window = builder.build()?;
    platform::prepare(&window)?;
    if let Some((position, size)) = place {
        window.set_position(position)?;
        // After moving: on another display the size is in that display's scale.
        window.set_size(size)?;
    }
    if hidden {
        window.show()?;
    }
    platform::created(&window)?;
    Ok(window)
}

/// Creates the main window.
pub fn create_main(app: &App) -> tauri::Result<()> {
    platform::starting();
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

/// Opens a window the size of this one, which shows `content` first (see [`take`]),
/// and returns its label. It goes next to this one, or where a tab was `dropped` outside the
/// windows (see [`tab_drag::place_dropped`]). `sessions`: this window's sessions that a tab in
/// `content` shows, which the new window may take (see [`SessionManager::offer`]). Async:
/// creating a window from a synchronous command deadlocks on Windows.
pub async fn open(
    app: AppHandle,
    window: WebviewWindow,
    windows: &Windows,
    session_manager: &SessionManager,
    content: WindowContent,
    sessions: Vec<SessionId>,
    dropped: Option<Dropped>,
) -> Result<String> {
    let label = format!("{LABEL_PREFIX}{}", windows.next.fetch_add(1, Ordering::Relaxed) + 1);
    session_manager.offer(window.label(), &sessions, &label)?;
    windows.inbox.lock().unwrap().opened(&label, content, sessions);
    let at_drop = || Some((tab_drag::place_dropped(&app, &window, dropped?)?, size(&window).ok()?));
    let place = at_drop().or_else(|| cascade(&window).ok());
    if let Err(e) = build(&app, &label, place) {
        windows.inbox.lock().unwrap().forget(&label);
        session_manager.withdraw_offers(&label, &Default::default());
        return Err(e.into());
    }
    changed(&app);
    Ok(label)
}

/// What this window is to show and hasn't taken yet: what it opened for, and tabs given to it
/// ([`send_tab`]), in order. Each is taken once.
pub fn take(window: WebviewWindow, windows: &Windows) -> Vec<WindowContent> {
    windows.inbox.lock().unwrap().take(window.label())
}

/// Closes this window, which has no tabs left, unless it is the last one or has been given a
/// tab it hasn't taken yet; returns whether it closes. From then on it takes no tabs.
pub fn close_empty(app: AppHandle, window: WebviewWindow, windows: &Windows) -> tauri::Result<bool> {
    let last = app.webview_windows().len() < 2;
    if !windows.inbox.lock().unwrap().close_empty(window.label(), last) {
        return Ok(false);
    }
    window.destroy()?;
    Ok(true)
}

/// The state of a window's maximize button as the system sees it: under the pointer, pressed
/// (Windows, see `win.rs`; the page draws the button).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, TS)]
pub struct MaximizeButtonState {
    pub hover: bool,
    pub pressed: bool,
}

/// Another window, as the tab menu lists it.
#[derive(Serialize, TS)]
pub struct WindowInfo {
    pub label: String,
    pub title: String,
}

/// The other windows, in the order they opened.
pub fn list(app: AppHandle, window: WebviewWindow) -> Vec<WindowInfo> {
    let mut others: Vec<WebviewWindow> = app.webview_windows().into_values().filter(|w| w.label() != window.label()).collect();
    others.sort_by_key(|w| order(w.label()));
    others.into_iter().map(|w| WindowInfo { title: w.title().unwrap_or_default(), label: w.label().to_owned() }).collect()
}

/// Names the window after its tab (the Window menu, the taskbar, and the tab menus of the
/// other windows show it).
pub fn set_title(app: AppHandle, window: WebviewWindow, title: String) -> tauri::Result<()> {
    if window.title()? != title {
        window.set_title(&title)?;
        changed(&app);
    }
    Ok(())
}

/// Gives a tab to window `target`, which takes it (see [`take`]) when it hears
/// `tab-received`, and comes forward. `at`: where in its page it was dropped (CSS pixels from
/// the left), to put it there among the tabs. `sessions`: this window's sessions that the tab
/// shows, which `target` may take (see [`SessionManager::offer`]). Fails with
/// `window.notFound` if it has closed or is closing.
#[allow(clippy::too_many_arguments)]
pub fn send_tab(
    app: AppHandle,
    window: WebviewWindow,
    windows: &Windows,
    session_manager: &SessionManager,
    target: String,
    tab: serde_json::Value,
    sessions: Vec<SessionId>,
    at: Option<f64>,
) -> Result<()> {
    let source = window.label().to_owned();
    let window = app.get_webview_window(&target).ok_or_else(|| Error::new("window.notFound"))?;
    {
        // Held across offering the sessions: the window doesn't start closing in between.
        let mut inbox = windows.inbox.lock().unwrap();
        if !inbox.accepts(&target) {
            return Err(Error::new("window.notFound"));
        }
        session_manager.offer(&source, &sessions, &target)?;
        inbox.give(&target, WindowContent::Tab { tab, at }, sessions);
    }
    events::emit_to(&app, target, events::TabReceived);
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

/// Tells window `target`, which gave this one a tab, that its pane `pane` couldn't move here
/// (its session is still there): the `pane-stayed` event.
pub fn pane_stayed(app: AppHandle, target: String, pane: u32) {
    events::emit_to(&app, target, events::PaneStayed(pane));
}

/// How many tabs quitting would end, in all the windows: each is asked (the `busy-query`
/// event, answered with [`busy_reply`]), since only its page knows which of its tabs
/// are busy. A window that doesn't answer in time (its page still loading) counts all the
/// sessions it shows.
pub async fn busy_count(app: AppHandle, windows: &Windows, sessions: &SessionManager) -> tauri::Result<u32> {
    let labels: HashSet<String> = app.webview_windows().into_keys().collect();
    let (id, answered) = windows.busy.lock().unwrap().ask(labels.clone());
    for label in labels {
        events::emit_to(&app, label, events::BusyQuery(id));
    }
    let _ = tokio::time::timeout(BUSY_TIMEOUT, answered).await;
    let Some((count, silent)) = windows.busy.lock().unwrap().finish(id) else { return Ok(0) };
    Ok(count + silent.iter().map(|label| sessions.owned(label)).sum::<u32>())
}

/// A window's answer to `busy-query`.
pub fn busy_reply(window: WebviewWindow, windows: &Windows, query: u64, count: u32) {
    windows.busy.lock().unwrap().reply(query, window.label(), count);
}

/// Plays the system's alert sound, for a terminal's bell (the bell sound setting).
pub fn bell(app: AppHandle) {
    let _ = app.run_on_main_thread(platform::bell);
}

/// Runs `work` on the main thread, where the platform's window calls are made, and returns
/// what it gives. Not from the main thread itself, which would wait for itself: from async
/// commands and tasks.
async fn on_main_thread<T: Send + 'static>(app: &AppHandle, work: impl FnOnce() -> T + Send + 'static) -> tauri::Result<T> {
    let (done, result) = oneshot::channel();
    app.run_on_main_thread(move || drop(done.send(work())))?;
    result.await.map_err(|_| tauri::Error::FailedToReceiveMessage)
}

/// Quits, once the user has confirmed (or nothing was busy).
pub fn quit(app: AppHandle) {
    app.exit(0);
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
pub async fn title_double_click(window: WebviewWindow) -> tauri::Result<()> {
    match on_main_thread(window.app_handle(), platform::double_click_action).await? {
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

/// Shows the window's system menu (Restore, Move, Size, Minimize, Maximize, Close) at the
/// pointer, as a right click on a native title bar does. Windows and Linux.
pub fn system_menu(window: WebviewWindow) -> tauri::Result<()> {
    platform::system_menu(window)
}

/// A file dialog of the dialog plugin is opening (`open`) or has closed, for showing the
/// pointer in it on Windows (see `win::file_dialog`).
pub fn file_dialog(window: WebviewWindow, open: bool) -> tauri::Result<()> {
    platform::file_dialog(window, open)
}

/// Where files can be dragged out of the window (the SFTP file list); `None` when there is
/// none. A press there while the window is in the background doesn't bring the window
/// forward (the frontend does once it is released without dragging; see `macos.rs`). macOS
/// only: on Windows the web view always brings it forward, and a drag puts it back
/// ([`keep_behind`]).
pub fn set_drag_area(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    platform::set_drag_area(window, rect)
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
pub async fn buttons(app: AppHandle) -> tauri::Result<Vec<WindowButton>> {
    on_main_thread(&app, platform::buttons).await
}

/// Sets the window's appearance (all windows' on Linux, where it is GTK's); `None` follows
/// the system's. Off the main thread: on Linux it may ask the settings portal.
pub async fn set_theme(window: WebviewWindow, theme: Option<tauri::Theme>) -> tauri::Result<()> {
    platform::set_theme(&window, theme)
}

/// The title bar's color (`#rrggbb`), for the window's border while it is snapped (see
/// [`update_border`]); set with the theme. Windows only.
pub fn set_snapped_border_color(window: WebviewWindow, color: String) -> tauri::Result<()> {
    platform::set_snapped_border_color(window, color)
}

/// A part of the page (the maximize button, the drag area), in CSS pixels from the top left
/// of the window.
pub use crate::tab_drag::Rect;

/// Places the native window that makes the frontend's maximize button behave like a native
/// one (the Windows 11 snap layouts), over `rect`; `None` hides it. Windows only.
pub fn set_maximize_button(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    platform::set_maximize_button(window, rect)
}

/// After the window moved or was resized: Windows 11 frames a window that isn't maximized
/// with a 1px border outside the web view, which stays when the window is snapped to part of
/// the screen, where it looks like a gap along the screen's edges. The space can't be given
/// to the web view without taking over the frame's layout from Tao, so while snapped the
/// border takes the title bar's color and blends in. Windows only.
pub fn update_border(window: &tauri::Window) {
    platform::update_border(window);
}
