//! The events the backend sends the pages. Each is a type that names the event and carries its
//! payload, so that a name can't be sent with another event's payload, and the frontend's
//! listeners are typed from the same list (`Events` in `src/ipc/bindings.ts`). Sessions'
//! output and events go through their own channel instead (see `session/`).

use serde::Serialize;
use tauri::{Emitter, EventTarget, Runtime, WebviewWindow};
use ts_rs::TS;

/// An event the backend sends the pages.
pub trait Event {
    const NAME: &'static str;
    type Payload: Serialize + Clone;
    fn payload(self) -> Self::Payload;
}

/// Sends `event` to every window.
pub fn emit<R: Runtime, E: Event>(emitter: &impl Emitter<R>, event: E) {
    let _ = emitter.emit(E::NAME, event.payload());
}

/// Sends `event` to window `label` only.
pub fn emit_to<R: Runtime, E: Event>(emitter: &impl Emitter<R>, label: impl Into<String>, event: E) {
    let _ = emitter.emit_to(EventTarget::webview_window(label), E::NAME, event.payload());
}

/// Sends `event` to the windows other than `from`.
pub fn tell_others<E: Event>(from: &WebviewWindow, event: E) {
    let label = from.label().to_owned();
    let others = |target: &EventTarget| matches!(target, EventTarget::WebviewWindow { label: other } if *other != label);
    let _ = from.emit_filter(E::NAME, event.payload(), others);
}

/// Declares the events: `Name = "name";` without a payload (sent as `null`), `Name(Payload) =
/// "name";` with one. Also lists them for the bindings.
macro_rules! events {
    (@list [$($done:ident)*]) => {
        /// Each event's name and the TypeScript type of its payload, for the bindings; their
        /// payloads' types are declared through `visit`.
        #[cfg(test)]
        pub(crate) fn declarations(cfg: &ts_rs::Config) -> Vec<(&'static str, String)> {
            vec![$((<$done as Event>::NAME, <<$done as Event>::Payload as ts_rs::TS>::name(cfg))),*]
        }

        #[cfg(test)]
        pub(crate) fn visit(visitor: &mut impl ts_rs::TypeVisitor) {
            $(visitor.visit::<<$done as Event>::Payload>();)*
        }
    };
    (@list [$($done:ident)*] $(#[$meta:meta])* $event:ident = $name:literal; $($rest:tt)*) => {
        $(#[$meta])*
        pub struct $event;

        impl Event for $event {
            const NAME: &'static str = $name;
            type Payload = ();
            fn payload(self) {}
        }

        events!(@list [$($done)* $event] $($rest)*);
    };
    (@list [$($done:ident)*] $(#[$meta:meta])* $event:ident($payload:ty) = $name:literal; $($rest:tt)*) => {
        $(#[$meta])*
        pub struct $event(pub $payload);

        impl Event for $event {
            const NAME: &'static str = $name;
            type Payload = $payload;
            fn payload(self) -> $payload {
                self.0
            }
        }

        events!(@list [$($done)* $event] $($rest)*);
    };
    ($($events:tt)*) => {
        events!(@list [] $($events)*);
    };
}

/// What a window saved that the other windows show, which they read again when they hear of
/// it (the `config-changed` event).
#[derive(Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    Settings,
    /// Sessions, folders and proxies.
    Profiles,
    QuickCommands,
    Vault,
    AppLock,
}

events! {
    /// The app locked: by a window, when the system was idle, or when it locked its screen.
    AppLocked = "app-locked";
    /// Another window saved this, which this one reads again.
    ConfigChanged(Change) = "config-changed";
    /// Quit was chosen in the native menu (macOS): the focused window asks first.
    QuitRequested = "quit-requested";
    /// An item of the native menu (macOS) was chosen, by the id the page gave it.
    MenuAction(String) = "menu-action";
    /// What checking for updates found.
    UpdateStatus(crate::update::Status) = "update-status";
    /// A screen reader started or stopped.
    ScreenReaderChanged(bool) = "screen-reader-changed";
    /// Windows opened, closed or changed their titles.
    WindowsChanged = "windows-changed";
    /// A window has gone, by its label.
    WindowClosed(String) = "window-closed";
    /// The window was given a tab, which it takes with `window_take`.
    TabReceived = "tab-received";
    /// A pane of the tab this window sent to another couldn't move there, by its key here.
    PaneStayed(u32) = "pane-stayed";
    /// Sessions of this window offered to another for a tab given to it, whose page went
    /// before taking them: they stay here.
    OffersWithdrawn(Vec<crate::session::SessionId>) = "offers-withdrawn";
    /// How many of its tabs quitting would end, asked of each window by an id that its
    /// answer (`window_busy_reply`) gives back.
    BusyQuery(u64) = "busy-query";
    /// Where a tab dragged from another window would go in this one's tab bar, in CSS pixels
    /// from the left; `null` once it would go elsewhere.
    TabDragOver(Option<f64>) = "tab-drag-over";
    /// A system drag of a tab out of this window ended (Linux).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    TabDragNativeEnd(crate::tab_drag::TabDragNativeEnd) = "tab-drag-native-end";
    /// The desktop's layout of the window buttons changed (Linux).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    WindowButtonsChanged = "window-buttons-changed";
    /// The state of the maximize button as the system sees it (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    MaximizeButton(crate::window::MaximizeButtonState) = "window-maximize-button";
}
