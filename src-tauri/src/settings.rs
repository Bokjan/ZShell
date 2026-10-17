//! The app-wide preferences (`crate::model::Settings`), persisted as `settings.json` next to
//! `vault.json`. The backend stores and validates them.

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use crate::error::Result;
use crate::model::Settings;
use crate::persist::{JsonFile, SetAside};

pub struct SettingsStore {
    file: JsonFile,
    /// What queries read (some on the main thread), replaced once a change is written: they
    /// never wait for the disk.
    settings: RwLock<Settings>,
    /// Held by a change from writing the file to applying it: one at a time.
    writing: Mutex<()>,
}

impl SettingsStore {
    /// Missing or unreadable files fall back to the defaults (see [`JsonFile::load`]).
    pub fn load(path: PathBuf, set_aside: &SetAside) -> Self {
        let (file, settings) = JsonFile::load::<Settings>(path, set_aside);
        Self { file, settings: RwLock::new(settings.normalize()), writing: Mutex::new(()) }
    }

    pub fn get(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    /// Validates, stores and returns the settings actually in effect. `apply` puts them into
    /// effect (the backend's language) before the next change is stored, so that what is in
    /// effect is what was stored last, whichever window changed it.
    pub fn set(&self, settings: Settings, apply: impl FnOnce(&Settings)) -> Result<Settings> {
        let settings = settings.normalize();
        let _writing = self.writing.lock().unwrap();
        let current = self.get();
        self.file.write_changed(&current.stored(), &settings.stored())?;
        *self.settings.write().unwrap() = settings.clone();
        apply(&settings);
        Ok(settings)
    }
}
