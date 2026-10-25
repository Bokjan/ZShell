//! Linux: a GTK drag of files (`text/uri-list`) in a temporary folder, from the window. They
//! are downloaded there when another application asks for them, with the main loop running
//! meanwhile so that the window keeps working; that application then copies them. GTK 4
//! applications (Files, the desktop) ask as soon as the drag is over them, to decide whether
//! they take it, and show it refused until they have the files: so they are downloaded then,
//! before the drop, and a large download keeps the drag refused for a while.
//!
//! The web view asks for the files too while the drag is over it (WebKit, and wry for its
//! drag events): those requests, while the drag is over the web view, get no files. A drop
//! back on the window (which the frontend may take as moving into a folder) is told by the
//! event that makes the web view's drag leave: the drop itself rather than the pointer going.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use gtk::gdk::DragAction;
use gtk::glib::{self, MainContext, Propagation, SignalHandlerId};
use gtk::prelude::*;
use tauri::{Manager, WebviewWindow};
use tokio::sync::oneshot;

use super::{placeholders, DragResult, DropDownload, Item, Outcome};
use crate::error::{Error, Result};

const FETCH_IDLE: u8 = 0;
const FETCH_RUNNING: u8 = 1;
const FETCH_OK: u8 = 2;
const FETCH_FAILED: u8 = 3;

/// The items and their downloads, shared with the download task.
struct Shared {
    /// Each remote item and where it goes in the temporary folder.
    items: Vec<(String, PathBuf)>,
    download: Arc<DropDownload>,
    fetch: AtomicU8,
}

impl Shared {
    /// Downloads the items, once, running the main loop until done; whether that succeeded.
    fn fetch(self: &Arc<Self>) -> bool {
        let context = MainContext::default();
        if self.fetch.compare_exchange(FETCH_IDLE, FETCH_RUNNING, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            let this = self.clone();
            let waker = context.clone();
            tauri::async_runtime::spawn(async move {
                let ok = this.download.fetch(&this.items).await.is_ok();
                this.fetch.store(if ok { FETCH_OK } else { FETCH_FAILED }, Ordering::Release);
                waker.wakeup();
            });
        }
        loop {
            match self.fetch.load(Ordering::Acquire) {
                FETCH_OK => return true,
                FETCH_FAILED => return false,
                _ => {
                    context.iteration(true);
                }
            }
        }
    }

    /// The items' `file:` URIs, which only absolute paths have (the cache folder is relative
    /// under a relative HOME).
    fn uris(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|(_, local)| glib::filename_to_uri(std::path::absolute(local).ok()?, None).ok().map(String::from))
            .collect()
    }
}

/// How the drag is going, on the main thread.
#[derive(Default)]
struct Drag {
    /// The drag is over the web view.
    over_view: Cell<bool>,
    /// Where the drag was last over the web view, in CSS pixels.
    position: Cell<(i32, i32)>,
    dropped_inside: Cell<bool>,
    /// Another application asked for the files.
    asked: Cell<bool>,
    failed: Cell<bool>,
    handlers: RefCell<Vec<(glib::Object, SignalHandlerId)>>,
}

impl Drag {
    fn result(&self) -> DragResult {
        let (x, y) = self.position.get();
        let (inside, asked) = (self.dropped_inside.get(), self.asked.get());
        DragResult::ended(inside || asked, inside, asked && !self.failed.get(), f64::from(x), f64::from(y))
    }

    fn disconnect(&self) {
        for (object, handler) in self.handlers.take() {
            object.disconnect(handler);
        }
    }
}

/// Runs the drag from the main thread (the mouse button must still be down: the system
/// takes the drag from the press); `done` gets how it ended.
pub(super) fn start(
    window: &WebviewWindow,
    items: Vec<Item>,
    download: Arc<DropDownload>,
    done: oneshot::Sender<DragResult>,
) -> Result<()> {
    let root = crate::scratch_dir(window.app_handle()).map_err(Error::from)?.join("ZShell-drag");
    let icon = match items.as_slice() {
        [item] if item.is_dir => "folder",
        [_] => "text-x-generic",
        _ => "edit-copy",
    };
    let items = placeholders(&root, &download.transfer_id, &items)?;
    let shared = Arc::new(Shared { items, download, fetch: AtomicU8::new(FETCH_IDLE) });
    let window_ = window.clone();
    window
        .run_on_main_thread(move || {
            let done = RefCell::new(Some(done));
            let finish = move |result: DragResult| {
                if let Some(done) = done.borrow_mut().take() {
                    let _ = done.send(result);
                }
            };
            if let Err(error) = begin(&window_, shared, icon, finish) {
                eprintln!("drag out: {error}");
            }
        })
        .map_err(Error::from)
}

/// Starts the drag; `finish` is called once it has ended (and not at all if it couldn't
/// start, when dropping it cancels the drag).
fn begin(window: &WebviewWindow, shared: Arc<Shared>, icon: &'static str, finish: impl Fn(DragResult) + 'static) -> tauri::Result<()> {
    let gtk_window = window.gtk_window()?;
    let drag = Rc::new(Drag::default());

    // Where the drag is over the web view: GTK's widget coordinates are CSS pixels.
    if let Some(webview) = crate::window::web_view(&gtk_window) {
        let drag_ = drag.clone();
        let id = webview.connect_drag_motion(move |_, _, x, y, _| {
            drag_.over_view.set(true);
            drag_.position.set((x, y));
            false
        });
        drag.handlers.borrow_mut().push((webview.clone().upcast(), id));
        let drag_ = drag.clone();
        // GTK leaves the web view before dropping on it, within the drop's event.
        let id = webview.connect_drag_leave(move |_, _, _| {
            drag_.over_view.set(false);
            if gtk::current_event().is_some_and(|event| event.event_type() == gtk::gdk::EventType::DropStart) {
                drag_.dropped_inside.set(true);
            }
        });
        drag.handlers.borrow_mut().push((webview.clone().upcast(), id));
    }

    let drag_ = drag.clone();
    let shared_ = shared.clone();
    let id = gtk_window.connect_drag_data_get(move |_, _, data, _, _| {
        if drag_.over_view.get() || drag_.dropped_inside.get() {
            return;
        }
        drag_.asked.set(true);
        if shared_.fetch() {
            let uris = shared_.uris();
            data.set_uris(&uris.iter().map(String::as_str).collect::<Vec<_>>());
        }
    });
    drag.handlers.borrow_mut().push((gtk_window.clone().upcast(), id));
    let drag_ = drag.clone();
    let id = gtk_window.connect_drag_failed(move |_, _, _| {
        drag_.failed.set(true);
        Propagation::Proceed
    });
    drag.handlers.borrow_mut().push((gtk_window.clone().upcast(), id));
    let drag_ = drag.clone();
    let id = gtk_window.connect_drag_end(move |_, _| {
        drag_.disconnect();
        let mut result = drag_.result();
        // Still downloading (the drag ended in the main loop `fetch` runs): files asked for by
        // an application the drag then wasn't dropped on, and none has them, as each waits for
        // them before it is done with a drop.
        if shared.fetch.load(Ordering::Acquire) == FETCH_RUNNING {
            shared.download.cancelled.store(true, Ordering::Relaxed);
            if result.outcome == Outcome::Outside {
                result = DragResult::cancelled();
            }
        }
        finish(result);
    });
    drag.handlers.borrow_mut().push((gtk_window.clone().upcast(), id));

    let targets = gtk::TargetList::new(&[]);
    targets.add_uri_targets(0);
    let Some(context) = gtk_window.drag_begin_with_coordinates(&targets, DragAction::COPY, 1, None, -1, -1) else {
        drag.disconnect();
        return Ok(());
    };
    context.drag_set_icon_name(icon, 0, 0);
    Ok(())
}
