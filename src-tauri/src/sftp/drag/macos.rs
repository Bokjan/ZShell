//! macOS: each item is a file promise (`NSFilePromiseProvider`). The application it is dropped
//! on asks for it with the place to write it, on an operation queue of ours, and waits for
//! the completion handler, showing the file as in progress meanwhile.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent,
    NSEventModifierFlags, NSEventType, NSFilePromiseProvider, NSFilePromiseProviderDelegate, NSView, NSWindow, NSWorkspace,
};
use objc2_foundation::{
    ns_string, NSArray, NSError, NSNumber, NSOperationQueue, NSPoint, NSProcessInfo, NSRect, NSSize, NSString, NSURL,
};
use tauri::WebviewWindow;
use tokio::sync::oneshot;

use super::{DragResult, DropDownload, Item, Outcome};
use crate::error::{Error, Result};
use crate::local_name::local_file_name;

const ICON_SIZE: f64 = 32.0;

/// Promise delegates whose drag or downloads are not over; promise providers only hold their
/// delegate weakly.
static PROMISES: Mutex<Vec<Retained<PromiseDelegate>>> = Mutex::new(Vec::new());

/// Promise delegates let go of, and when. The last download of a drop lets go of its delegate
/// from inside the delegate's own `write_promise`, on a thread of the receiving application's
/// choosing, which must not free it while it runs: they are freed a while later.
static RELEASED: Mutex<Vec<(Instant, Retained<PromiseDelegate>)>> = Mutex::new(Vec::new());

/// How long a let-go promise delegate is kept, far longer than its `write_promise` takes to
/// return.
const RELEASE_DELAY: Duration = Duration::from_secs(5);

thread_local! {
    /// Drag sources of drags in progress (main thread only).
    static SOURCES: RefCell<Vec<Retained<DragSource>>> = const { RefCell::new(Vec::new()) };
}

struct PromiseIvars {
    items: Vec<Item>,
    download: Arc<DropDownload>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `PromiseDelegate` does not
    // implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "ZShellFilePromiseDelegate"]
    #[ivars = PromiseIvars]
    struct PromiseDelegate;

    unsafe impl NSObjectProtocol for PromiseDelegate {}

    unsafe impl NSFilePromiseProviderDelegate for PromiseDelegate {
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
        fn file_name(&self, provider: &NSFilePromiseProvider, _file_type: &NSString) -> Retained<NSString> {
            // As for every local name (`local_file_name`); an item never has a name it
            // refuses (`..`, a `/`), and Finder would make it safe anyway.
            let name = self.item(provider).map_or("", Item::name);
            NSString::from_str(&local_file_name(name).unwrap_or_else(|| name.to_owned()))
        }

        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
        fn write_promise(&self, provider: &NSFilePromiseProvider, url: &NSURL, completion: &DynBlock<dyn Fn(*mut NSError)>) {
            let download = self.ivars().download.clone();
            let result = match (self.item(provider), url.to_file_path()) {
                (Some(item), Some(local)) => {
                    let remote = item.path.clone();
                    let download = download.clone();
                    tauri::async_runtime::block_on(async move { download.fetch(&remote, local).await })
                }
                _ => Err(Error::new("transfer.invalidPath").param("path", url_text(url))),
            };
            // The last download may have let go of `self` (`release`), which stays until
            // this returns but is not used again.
            match result {
                Ok(()) => completion.call((std::ptr::null_mut(),)),
                Err(_) => {
                    let error = ns_error();
                    completion.call((Retained::as_ptr(&error) as *mut NSError,));
                }
            }
        }

        // Downloads block the thread they are asked for on, so not the main thread.
        #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
        fn operation_queue(&self, _provider: &NSFilePromiseProvider) -> Retained<NSOperationQueue> {
            NSOperationQueue::new()
        }
    }
);

impl PromiseDelegate {
    fn item(&self, provider: &NSFilePromiseProvider) -> Option<&Item> {
        let info = provider.userInfo()?;
        let index = info.downcast::<NSNumber>().ok()?.unsignedIntegerValue();
        self.ivars().items.get(index)
    }

}

/// Lets go of the promise delegate of a drop that is over (see `RELEASED`).
pub(super) fn release(download: &DropDownload) {
    let released: Vec<_> = {
        let mut promises = PROMISES.lock().unwrap();
        let (released, kept) =
            promises.drain(..).partition(|delegate| std::ptr::eq(Arc::as_ptr(&delegate.ivars().download), download));
        *promises = kept;
        released
    };
    if released.is_empty() {
        return;
    }
    let now = Instant::now();
    RELEASED.lock().unwrap().extend(released.into_iter().map(|delegate| (now, delegate)));
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(RELEASE_DELAY).await;
        let freed: Vec<_> = {
            let mut released = RELEASED.lock().unwrap();
            let (freed, kept) = released.drain(..).partition(|(at, _)| at.elapsed() >= RELEASE_DELAY);
            *released = kept;
            freed
        };
        drop(freed);
    });
}

struct SourceIvars {
    promises: Retained<PromiseDelegate>,
    /// The window's frame on screen, to tell drops on it apart and place them in the page.
    frame: NSRect,
    /// The window's number, to tell drops on it from drops on what is in front of it.
    window_number: isize,
    /// To let go of the source once its `ended` has returned.
    window: WebviewWindow,
    done: RefCell<Option<oneshot::Sender<DragResult>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `DragSource` does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZShellDragSource"]
    #[ivars = SourceIvars]
    struct DragSource;

    unsafe impl NSObjectProtocol for DragSource {}

    unsafe impl NSDraggingSource for DragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, point: NSPoint, operation: NSDragOperation) {
            let frame = self.ivars().frame;
            // In the window's frame, and on the window rather than on something in front of
            // it (the Dock over a window as tall as the screen, a floating panel).
            let inside = point.x >= frame.origin.x
                && point.x <= frame.origin.x + frame.size.width
                && point.y >= frame.origin.y
                && point.y <= frame.origin.y + frame.size.height
                && NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(point, 0, self.mtm()) == self.ivars().window_number;
            let outcome = if operation == NSDragOperation::None {
                Outcome::Cancelled
            } else if inside {
                Outcome::Inside
            } else {
                Outcome::Outside
            };
            // The page fills the window (the title bar overlays it); screen coordinates grow
            // upwards.
            let result = DragResult { outcome, x: point.x - frame.origin.x, y: frame.origin.y + frame.size.height - point.y };
            if let Some(done) = self.ivars().done.borrow_mut().take() {
                let _ = done.send(result);
            }
            if outcome != Outcome::Outside {
                release(&self.ivars().promises.ivars().download);
            }
            // Not freed while running: once this has returned (the main thread runs one thing
            // at a time).
            let this = self as *const Self as usize;
            let _ = self.ivars().window.run_on_main_thread(move || {
                SOURCES.with_borrow_mut(|sources| sources.retain(|source| Retained::as_ptr(source) as usize != this));
            });
        }
    }
);

fn url_text(url: &NSURL) -> String {
    url.absoluteString().map(|s| s.to_string()).unwrap_or_default()
}

/// The receiving application shows its own message; ours is in the transfer list.
fn ns_error() -> Retained<NSError> {
    unsafe { NSError::errorWithDomain_code_userInfo(ns_string!("ZShell"), 1, None) }
}

/// Starts the drag session on the main thread; `done` gets how it ended.
pub(super) fn start(
    window: &WebviewWindow,
    items: Vec<Item>,
    download: Arc<DropDownload>,
    done: oneshot::Sender<DragResult>,
) -> Result<()> {
    let owner = window.clone();
    window
        .with_webview(move |webview| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            // SAFETY: on macOS the platform webview is a WKWebView, an NSView.
            let view: &NSView = unsafe { &*(webview.inner() as *const NSView) };
            let Some(ns_window) = view.window() else { return };
            let in_window = ns_window.convertPointFromScreen(NSEvent::mouseLocation());
            // The press that started the drag was taken by the page, so a drag event is made
            // up for where the mouse is now.
            let Some(event) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                NSEventType::LeftMouseDragged,
                in_window,
                NSEventModifierFlags::empty(),
                NSProcessInfo::processInfo().systemUptime(),
                ns_window.windowNumber(),
                None,
                0,
                1,
                1.0,
            ) else {
                return;
            };

            let promises = PromiseDelegate::alloc().set_ivars(PromiseIvars { items, download });
            let promises: Retained<PromiseDelegate> = unsafe { msg_send![super(promises), init] };
            // Icons stacked at the pointer, in the view's coordinates.
            let at = view.convertPoint_fromView(in_window, None);
            let workspace = NSWorkspace::sharedWorkspace();
            let dragging_items: Vec<Retained<NSDraggingItem>> = promises
                .ivars()
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let file_type = if item.is_dir { ns_string!("public.folder") } else { ns_string!("public.data") };
                    let provider = NSFilePromiseProvider::initWithFileType_delegate(
                        NSFilePromiseProvider::alloc(),
                        file_type,
                        ProtocolObject::from_ref(&*promises),
                    );
                    let number = NSNumber::new_usize(index);
                    unsafe { provider.setUserInfo(Some(&number)) };
                    let dragging =
                        NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*provider));
                    // Deprecated for iconForContentType:, which needs the UniformTypeIdentifiers
                    // framework; this one still takes an extension or a type identifier.
                    #[allow(deprecated)]
                    let icon = if item.is_dir {
                        workspace.iconForFileType(file_type)
                    } else {
                        workspace.iconForFileType(&NSString::from_str(item.name().rsplit_once('.').map_or("", |(_, ext)| ext)))
                    };
                    let offset = (index.min(4) * 4) as f64;
                    let rect = NSRect::new(
                        NSPoint::new(at.x - ICON_SIZE / 2.0 + offset, at.y - ICON_SIZE / 2.0 - offset),
                        NSSize::new(ICON_SIZE, ICON_SIZE),
                    );
                    let contents: &AnyObject = icon.as_ref();
                    unsafe { dragging.setDraggingFrame_contents(rect, Some(contents)) };
                    dragging
                })
                .collect();

            let source = DragSource::alloc(mtm).set_ivars(SourceIvars {
                promises: promises.clone(),
                frame: ns_window.frame(),
                window_number: ns_window.windowNumber(),
                window: owner,
                done: RefCell::new(Some(done)),
            });
            let source: Retained<DragSource> = unsafe { msg_send![super(source), init] };
            PROMISES.lock().unwrap().push(promises);
            SOURCES.with_borrow_mut(|sources| sources.push(source.clone()));
            view.beginDraggingSessionWithItems_event_source(
                &NSArray::from_retained_slice(&dragging_items),
                &event,
                ProtocolObject::from_ref(&*source),
            );
        })
        .map_err(Error::from)
}
