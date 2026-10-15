//! The app-wide preferences (`crate::model::Settings`), persisted as `settings.json` next to
//! `vault.json`. The backend stores and validates them.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::Result;
use crate::model::Settings;
use crate::persist::{JsonFile, SetAside};

pub struct SettingsStore {
    file: JsonFile,
    settings: Mutex<Settings>,
}

impl SettingsStore {
    /// Missing or unreadable files fall back to the defaults (see [`JsonFile::load`]).
    pub fn load(path: PathBuf, set_aside: &SetAside) -> Self {
        let (file, settings) = JsonFile::load::<Settings>(path, set_aside);
        Self { file, settings: Mutex::new(settings.normalize()) }
    }

    pub fn get(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Validates, stores and returns the settings actually in effect.
    pub fn set(&self, settings: Settings) -> Result<Settings> {
        let settings = settings.normalize();
        let mut current = self.settings.lock().unwrap();
        self.file.write_changed(&current.stored(), &settings.stored())?;
        *current = settings.clone();
        Ok(settings)
    }
}
