//! Commands for the debug log and its window.

use tauri::{AppHandle, WebviewWindow};

use crate::debug_log::{self, FrontendRecord, LogEntry, LogSource};
use crate::error::Result;
use crate::window;

/// Opens the debug log window, or brings it forward.
#[tauri::command]
pub async fn debug_log_open(app: AppHandle) -> Result<()> {
    Ok(window::open_debug_log(app).await?)
}

/// Both sides' records, oldest first; from then on the window is sent the new ones
/// (`debug-log-added`), until its page goes.
#[tauri::command]
pub fn debug_log_snapshot() -> Vec<LogEntry> {
    debug_log::snapshot()
}

/// Forgets one side's records.
#[tauri::command]
pub fn debug_log_clear(source: LogSource) {
    debug_log::clear(source);
}

/// Records what this window's page reports.
#[tauri::command]
pub fn debug_log_write(window: WebviewWindow, records: Vec<FrontendRecord>) {
    debug_log::write(window.label(), records);
}
