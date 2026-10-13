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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_char;
use std::sync::{Once, OnceLock};

use objc2::ffi::{class_replaceMethod, method_getTypeEncoding};
use objc2::runtime::{AnyClass, Bool, Imp, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEvent, NSView};

use super::Rect;

// Only touched on the main thread, where AppKit asks about presses.
thread_local! {
    /// Each web view's drag area (by the view's address), in its CSS pixels.
    static AREAS: RefCell<HashMap<usize, Rect>> = RefCell::new(HashMap::new());
    /// The press being handled is on a drag area, with the window's ordering delayed.
    static DELAYED: Cell<bool> = const { Cell::new(false) };
}

type EventQuery = unsafe extern "C-unwind" fn(&NSView, Sel, &NSEvent) -> Bool;
type EventHandler = unsafe extern "C-unwind" fn(&NSView, Sel, &NSEvent);

/// The web view class's own methods.
static ACCEPTS_FIRST_MOUSE: OnceLock<EventQuery> = OnceLock::new();
static SHOULD_DELAY: OnceLock<EventQuery> = OnceLock::new();
static MOUSE_DOWN: OnceLock<EventHandler> = OnceLock::new();

/// Sets the drag area of `view`, a web view; `None` removes it.
pub(super) fn set_area(view: &NSView, rect: Option<Rect>) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| install(view.class()));
    AREAS.with_borrow_mut(|areas| match rect {
        Some(rect) => areas.insert(view as *const NSView as usize, rect),
        None => areas.remove(&(view as *const NSView as usize)),
    });
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
    let Some(rect) = AREAS.with_borrow(|areas| areas.get(&(view as *const NSView as usize)).copied()) else { return false };
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
