//! Platforms without an app lock yet. On Linux it is planned with polkit, Mutter's idle monitor
//! and logind (see M31 in `docs/ROADMAP.md`).

use tauri::WebviewWindow;

use super::{Method, System};
use crate::error::{Error, Result};

pub struct Platform;

impl System for Platform {
    fn method(&self) -> Result<Method> {
        Err(Error::new("appLock.unsupported"))
    }

    fn authenticate(&self, _window: Option<&WebviewWindow>) -> Result<()> {
        Err(Error::new("appLock.unsupported"))
    }

    fn idle_seconds(&self) -> Option<u64> {
        None
    }

    fn watches_system_lock(&self) -> bool {
        false
    }
}

pub fn watch_system(_on_lock: Box<dyn Fn() + Send + Sync>) {}
