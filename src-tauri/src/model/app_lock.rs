//! The app lock's settings, kept by the vault with the sessions (see `crate::applock`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The idle times that can be chosen, in minutes; 0 never locks.
const IDLE_MINUTES: [u32; 6] = [0, 1, 5, 15, 30, 60];
pub(crate) const DEFAULT_IDLE_MINUTES: u32 = 15;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct AppLockSettings {
    pub enabled: bool,
    /// Locks after the system (not only ZShell) has had no input for this long; 0 never.
    pub idle_minutes: u32,
    /// Locks when the system locks its screen, sleeps or switches to another user.
    pub on_system_lock: bool,
}

impl Default for AppLockSettings {
    fn default() -> Self {
        Self { enabled: false, idle_minutes: DEFAULT_IDLE_MINUTES, on_system_lock: true }
    }
}

impl AppLockSettings {
    pub(crate) fn normalize(mut self) -> Self {
        if !IDLE_MINUTES.contains(&self.idle_minutes) {
            self.idle_minutes = DEFAULT_IDLE_MINUTES;
        }
        self
    }
}
