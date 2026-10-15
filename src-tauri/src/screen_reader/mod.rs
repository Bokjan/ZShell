//! Whether the system's screen reader is running (VoiceOver, Narrator, NVDA, JAWS, Orca),
//! which the web view can't tell: terminals are read for screen readers while one is, when the
//! setting says so (`ScreenReaderMode::Auto`). Each window hears of changes as they happen
//! (`screen-reader-changed`).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
use win as platform;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

/// Called with whether a screen reader is running: first as watching starts, then on changes.
type Changed = Box<dyn Fn(bool) + Send + Sync>;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether a screen reader is running, as last told; not running until watching has started.
pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Starts watching, from the main thread.
pub fn watch(app: AppHandle) {
    platform::watch(Box::new(move |running| {
        if RUNNING.swap(running, Ordering::Relaxed) != running {
            let _ = app.emit("screen-reader-changed", running);
        }
    }));
}
