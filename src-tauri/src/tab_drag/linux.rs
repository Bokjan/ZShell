//! Wayland: a tab dragged out of its window goes on as a GTK drag (of a type only the app
//! takes), since there a window gets no pointer events while another holds the pointer, and
//! the app knows neither the cursor's place on the screen nor its windows'. The windows' web
//! views take the drag: over another window's tab bar it shows where the tab would go, and a
//! drop there puts it there. Dropped where no window takes it, the tab opens in a window of
//! its own; Escape cancels. The page that dragged the tab hears the end
//! (`tab-drag-native-end`) and asks where it went ([`super::tab_drag_end`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk::{DragAction, DragContext};
use gtk::glib::{self, Propagation, SignalHandlerId};
use gtk::prelude::*;
use tauri::{AppHandle, Emitter, EventTarget, Manager, WebviewWindow};

use super::{DropTarget, Point, TabDrag, TabDragNativeEnd};

/// The drag's data type, which no other application takes.
const TARGET: &str = "application/x-zshell-tab";

/// How the drag is going, on the main thread.
struct Drag {
    id: String,
    source: String,
    /// Where it was dropped on a window.
    dropped: RefCell<Option<DropTarget>>,
    cancelled: Cell<bool>,
    handlers: RefCell<Vec<(glib::Object, SignalHandlerId)>>,
}

thread_local! {
    /// The drag going on, if any.
    static CURRENT: RefCell<Option<Rc<Drag>>> = const { RefCell::new(None) };
}

fn is_tab(context: &DragContext) -> bool {
    context.list_targets().iter().any(|atom| atom.name() == TARGET)
}

/// Where the tab would go over window `label` at `x`, `y` (its page's CSS pixels).
fn target(app: &AppHandle, source: &str, label: &str, x: i32, y: i32) -> DropTarget {
    let (x, y) = (f64::from(x), f64::from(y));
    if label == source {
        return DropTarget::Here;
    }
    match app.state::<TabDrag>().tab_bars.lock().unwrap().get(label) {
        Some(rect) if rect.contains(x, y) => DropTarget::Window { label: label.to_owned(), x },
        _ => DropTarget::Elsewhere { cursor: Point { x: 0.0, y: 0.0 } },
    }
}

fn show(app: &AppHandle, target: &DropTarget) {
    app.state::<TabDrag>().drags.lock().unwrap().show(app, target);
}

/// Starts the system drag of drag `id` from window `source` (on the main thread, with the
/// mouse button still down), unless it has started already; whether it goes on.
pub fn start(app: &AppHandle, source: &str, id: String) -> bool {
    if CURRENT.with(|current| current.borrow().as_ref().is_some_and(|drag| drag.id == id)) {
        return true;
    }
    let Some(gtk_window) = app.get_webview_window(source).and_then(|window| window.gtk_window().ok()) else { return false };
    let drag = Rc::new(Drag {
        id,
        source: source.to_owned(),
        dropped: RefCell::new(None),
        cancelled: Cell::new(false),
        handlers: RefCell::new(Vec::new()),
    });
    for window in app.webview_windows().into_values() {
        connect_window(app, &drag, &window);
    }

    let drag_ = drag.clone();
    let id = gtk_window.connect_drag_failed(move |_, _, result| {
        drag_.cancelled.set(result == gtk::DragResult::UserCancelled);
        // No animation of the tab going back: it may go into a window of its own.
        Propagation::Stop
    });
    drag.handlers.borrow_mut().push((gtk_window.clone().upcast(), id));
    let (drag_, app_) = (drag.clone(), app.clone());
    let id = gtk_window.connect_drag_end(move |_, _| end(&app_, &drag_));
    drag.handlers.borrow_mut().push((gtk_window.clone().upcast(), id));

    let targets = gtk::TargetList::new(&[gtk::TargetEntry::new(TARGET, gtk::TargetFlags::SAME_APP, 0)]);
    let Some(context) = gtk_window.drag_begin_with_coordinates(&targets, DragAction::MOVE, 1, None, -1, -1) else {
        disconnect(&drag);
        return false;
    };
    context.drag_set_icon_name("window-new", 0, 0);
    CURRENT.with(|current| *current.borrow_mut() = Some(drag));
    true
}

/// Lets `window`'s web view take the drag. Returning true from its handlers keeps WebKit out
/// of the drag (wry's handlers, which come first, find no files in it and let it through).
fn connect_window(app: &AppHandle, drag: &Rc<Drag>, window: &WebviewWindow) {
    let Some(webview) = window.gtk_window().ok().as_ref().and_then(crate::window::web_view) else { return };
    let label = window.label().to_owned();

    let (app_, drag_, label_) = (app.clone(), drag.clone(), label.clone());
    let id = webview.connect_drag_motion(move |_, context, x, y, time| {
        if !is_tab(context) {
            return false;
        }
        show(&app_, &target(&app_, &drag_.source, &label_, x, y));
        context.drag_status(DragAction::MOVE, time);
        true
    });
    drag.handlers.borrow_mut().push((webview.clone().upcast(), id));

    let app_ = app.clone();
    let id = webview.connect_drag_leave(move |_, context, _| {
        if is_tab(context) {
            show(&app_, &DropTarget::Here);
        }
    });
    drag.handlers.borrow_mut().push((webview.clone().upcast(), id));

    let (app_, drag_) = (app.clone(), drag.clone());
    let id = webview.connect_drag_drop(move |_, context, x, y, time| {
        if !is_tab(context) {
            return false;
        }
        *drag_.dropped.borrow_mut() = Some(target(&app_, &drag_.source, &label, x, y));
        context.drop_finish(true, time);
        true
    });
    drag.handlers.borrow_mut().push((webview.upcast(), id));
}

/// The drag ended: keeps where the tab goes for the page to ask, and tells it.
fn end(app: &AppHandle, drag: &Rc<Drag>) {
    disconnect(drag);
    CURRENT.with(|current| current.borrow_mut().take());
    show(app, &DropTarget::Here);
    let target = match drag.dropped.borrow_mut().take() {
        Some(target) => Some(target),
        None if drag.cancelled.get() => None,
        None => Some(DropTarget::Elsewhere { cursor: Point { x: 0.0, y: 0.0 } }),
    };
    let dropped = target.is_some();
    *app.state::<TabDrag>().native_ended.lock().unwrap() = Some((drag.id.clone(), target));
    let end = TabDragNativeEnd { id: drag.id.clone(), dropped };
    let _ = app.emit_to(EventTarget::webview_window(drag.source.clone()), "tab-drag-native-end", end);
}

fn disconnect(drag: &Drag) {
    for (object, handler) in drag.handlers.take() {
        object.disconnect(handler);
    }
}
