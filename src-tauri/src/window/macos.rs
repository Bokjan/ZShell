//! macOS: a press on a window's drag area (the SFTP file list) while the window is in the
//! background reaches the page without bringing the window, or the app, forward. Files
//! dragged out of it can then be dropped on the window in front, as from a background Finder
//! window; a press released without dragging has the frontend bring the window forward.
//!
//! AppKit delays ordering the window forward (and activating the app) for a view that asks
//! (`shouldDelayWindowOrderingForEvent:`) only until the view's `mouseDown:` returns, unless
//! that calls `preventWindowOrdering`; WebKit's returns at once, the page hearing of the press
//! later, and only delays the ordering for drags of selected text. So Wry's web view class
//! gets its own `acceptsFirstMouse:`, `shouldDelayWindowOrderingForEvent:` and `mouseDown:`,
//! which defer to the originals outside the area.
//!
//! Also the rest of macOS's part of `window` (see its module docs): the overlay title bar
//! with the traffic lights, what entering or leaving full screen does ([`watch_full_screen`]),
//! and what is kept for each window, which goes when it is destroyed ([`forget`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_char;
use std::sync::{Once, OnceLock};

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::ffi::{class_replaceMethod, method_getTypeEncoding};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEvent, NSView, NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter};
use tauri::{AppHandle, Manager, WebviewWindow, WebviewWindowBuilder, Wry};

use super::{DoubleClick, Rect, WindowButton};
use crate::model::Appearance;

// Only touched on the main thread, where AppKit asks about presses and posts the window's
// notifications.
thread_local! {
    /// Each web view's drag area (by the view's address), in its CSS pixels, with the label of
    /// its window.
    static AREAS: RefCell<HashMap<usize, (String, Rect)>> = RefCell::new(HashMap::new());
    /// Each window's full screen observers, by its label.
    static OBSERVERS: RefCell<HashMap<String, Vec<Observer>>> = RefCell::new(HashMap::new());
    /// The press being handled is on a drag area, with the window's ordering delayed.
    static DELAYED: Cell<bool> = const { Cell::new(false) };
}

/// The token for an observer of the notification center.
type Observer = Retained<ProtocolObject<dyn NSObjectProtocol>>;

type EventQuery = unsafe extern "C-unwind" fn(&NSView, Sel, &NSEvent) -> Bool;
type EventHandler = unsafe extern "C-unwind" fn(&NSView, Sel, &NSEvent);

/// The web view class's own methods.
static ACCEPTS_FIRST_MOUSE: OnceLock<EventQuery> = OnceLock::new();
static SHOULD_DELAY: OnceLock<EventQuery> = OnceLock::new();
static MOUSE_DOWN: OnceLock<EventHandler> = OnceLock::new();

/// Sets the drag area of `view`, window `label`'s web view; `None` removes it. On the main
/// thread.
fn set_area(view: &NSView, label: &str, rect: Option<Rect>) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| install(view.class()));
    AREAS.with_borrow_mut(|areas| match rect {
        Some(rect) => areas.insert(view as *const NSView as usize, (label.to_owned(), rect)),
        None => areas.remove(&(view as *const NSView as usize)),
    });
}

/// How long after a full screen transition the web view takes the focus back.
const REFOCUS_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

/// As the window enters or leaves full screen (its own or the terminals'): the menu bar's full
/// screen item says what it now does (`menu::show_full_screen`), and the web view gets the
/// keyboard focus back. Once the window has left full screen, the focus is on the window
/// rather than the web view, so keys go nowhere until it is clicked: the web view gets it
/// back shortly after either transition ends, since focusing it right away is undone by what
/// the window still does as the transition finishes.
fn watch_full_screen(window: &WebviewWindow) -> tauri::Result<()> {
    let window = window.clone();
    window.app_handle().clone().run_on_main_thread(move || {
        let Ok(ns_window) = window.ns_window() else { return };
        let center = NSNotificationCenter::defaultCenter();
        let mut observers = Vec::new();
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
            // SAFETY: `ns_window` is the window's NSWindow, which is alive now; the observer is
            // removed once the window is destroyed ([`forget`]), before the address can be
            // another window's.
            observers.push(unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), Some(&*ns_window.cast::<AnyObject>()), None, &block)
            });
        }
        OBSERVERS.with_borrow_mut(|all| all.insert(window.label().to_owned(), observers));
    })
}

/// Lets go of what was kept for `window`, now destroyed: its drag area and its full screen
/// observers. On the main thread.
pub(super) fn forget(window: &tauri::Window) {
    let label = window.label();
    AREAS.with_borrow_mut(|areas| areas.retain(|_, (owner, _)| owner != label));
    if let Some(observers) = OBSERVERS.with_borrow_mut(|all| all.remove(label)) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in observers {
            // SAFETY: a token the center gave for an observer it keeps.
            unsafe { center.removeObserver(observer.as_ref()) };
        }
    }
}

/// Replaces the methods on `class`, the web view's (Wry's subclass of `WKWebView`): `imp`
/// takes the place of `sel`, whose implementation is kept in `original`.
///
/// # Safety
/// `F` is the function type of the method, which `imp` is.
unsafe fn replace<F: Copy>(class: &AnyClass, sel: Sel, imp: F, original: &OnceLock<F>) {
    let Some(method) = class.instance_method(sel) else { return };
    let _ = original.set(std::mem::transmute_copy::<Imp, F>(&method.implementation()));
    let types: *const c_char = method_getTypeEncoding(method);
    // Inherited (`WKWebView`'s), the method is added to the subclass, and the superclass
    // keeps its own.
    class_replaceMethod(class as *const AnyClass as *mut AnyClass, sel, std::mem::transmute_copy::<F, Imp>(&imp), types);
}

fn install(class: &AnyClass) {
    // SAFETY: each function has the signature of the method it replaces, on the class AppKit
    // sends them to.
    unsafe {
        replace::<EventQuery>(class, sel!(acceptsFirstMouse:), accepts_first_mouse, &ACCEPTS_FIRST_MOUSE);
        replace::<EventQuery>(class, sel!(shouldDelayWindowOrderingForEvent:), should_delay_window_ordering, &SHOULD_DELAY);
        replace::<EventHandler>(class, sel!(mouseDown:), mouse_down, &MOUSE_DOWN);
    }
}

/// Whether `event`, a press, is on the drag area of `view`.
fn in_area(view: &NSView, event: &NSEvent) -> bool {
    let Some(rect) = AREAS.with_borrow(|areas| areas.get(&(view as *const NSView as usize)).map(|(_, rect)| *rect)) else { return false };
    let point = view.convertPoint_fromView(event.locationInWindow(), None);
    // The page's coordinates grow downwards.
    let y = if view.isFlipped() { point.y } else { view.bounds().size.height - point.y };
    point.x >= rect.x && point.x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

unsafe extern "C-unwind" fn accepts_first_mouse(this: &NSView, sel: Sel, event: &NSEvent) -> Bool {
    if in_area(this, event) {
        return Bool::YES;
    }
    match ACCEPTS_FIRST_MOUSE.get() {
        Some(original) => original(this, sel, event),
        None => Bool::NO,
    }
}

// Asked only of a press on a window in the background.
unsafe extern "C-unwind" fn should_delay_window_ordering(this: &NSView, sel: Sel, event: &NSEvent) -> Bool {
    if in_area(this, event) {
        DELAYED.set(true);
        return Bool::YES;
    }
    match SHOULD_DELAY.get() {
        Some(original) => original(this, sel, event),
        None => Bool::NO,
    }
}

unsafe extern "C-unwind" fn mouse_down(this: &NSView, sel: Sel, event: &NSEvent) {
    if DELAYED.replace(false) {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).preventWindowOrdering();
        }
    }
    if let Some(original) = MOUSE_DOWN.get() {
        original(this, sel, event);
    }
}

/// Where the traffic lights sit, centered in the frontend's 34px title bar.
const TRAFFIC_LIGHTS: tauri::LogicalPosition<f64> = tauri::LogicalPosition { x: 14.0, y: 19.0 };

/// The title bar: the page draws under it, the traffic lights over the page.
pub(super) fn title_bar(builder: WebviewWindowBuilder<'_, Wry, AppHandle>) -> WebviewWindowBuilder<'_, Wry, AppHandle> {
    builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true).traffic_light_position(TRAFFIC_LIGHTS)
}

/// The web view's own background until the page paints; only Windows sets one (see
/// `win::background`).
pub(super) fn background(
    builder: WebviewWindowBuilder<'_, Wry, AppHandle>,
    _appearance: Appearance,
) -> WebviewWindowBuilder<'_, Wry, AppHandle> {
    builder
}

/// Nothing to do before the window is first shown.
pub(super) const PREPARED_HIDDEN: bool = false;

pub(super) fn prepare(_window: &WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

/// Once the window is shown: follows it entering and leaving full screen.
pub(super) fn created(window: &WebviewWindow) -> tauri::Result<()> {
    watch_full_screen(window)
}

/// Nothing to set up before the first window.
pub(super) fn starting() {}

/// On the main thread.
pub(super) fn bell() {
    objc2_app_kit::NSBeep();
}

/// What a double click on the title bar does, as the system setting says ("Double-click a
/// window's title bar to"). On the main thread.
pub(super) fn double_click_action() -> DoubleClick {
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

/// No window menu: the title bar's right click does nothing on macOS.
pub(super) fn system_menu(_window: WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

/// File dialogs show the pointer by themselves.
pub(super) fn file_dialog(_window: WebviewWindow, _open: bool) -> tauri::Result<()> {
    Ok(())
}

/// See `window::set_drag_area`.
pub(super) fn set_drag_area(window: WebviewWindow, rect: Option<Rect>) -> tauri::Result<()> {
    let label = window.label().to_owned();
    // On the main thread, where the web view runs this.
    window.with_webview(move |webview| {
        // SAFETY: on macOS the platform webview is a WKWebView, an NSView.
        let view: &NSView = unsafe { &*(webview.inner() as *const NSView) };
        set_area(view, &label, rect);
    })
}

/// None: the traffic lights are the system's.
pub(super) fn buttons() -> Vec<WindowButton> {
    Vec::new()
}

pub(super) fn set_theme(window: &WebviewWindow, theme: Option<tauri::Theme>) -> tauri::Result<()> {
    window.set_theme(theme)
}

/// No border to color while snapped (Windows 11's).
pub(super) fn set_snapped_border_color(_window: WebviewWindow, _color: String) -> tauri::Result<()> {
    Ok(())
}

/// The snap layouts are Windows 11's.
pub(super) fn set_maximize_button(_window: WebviewWindow, _rect: Option<Rect>) -> tauri::Result<()> {
    Ok(())
}

pub(super) fn update_border(_window: &tauri::Window) {}
