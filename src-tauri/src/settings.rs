//! App-wide preferences (appearance, terminal), persisted as `settings.json` next to
//! `profiles.json`. Only the frontend interprets them; the backend stores and validates.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::config::write_json_atomic;
use crate::error::Result;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub appearance: Appearance,
    pub terminal: TerminalSettings,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalSettings {
    /// A built-in color scheme id, or "auto" to follow the appearance.
    pub color_scheme: String,
    /// Preferred font family; empty uses the platform default. Fallbacks (including CJK
    /// fonts) are always appended by the frontend.
    pub font_family: String,
    pub font_size: u16,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    /// Lines kept above the screen.
    pub scrollback: u32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

impl Default for Settings {
    fn default() -> Self {
        Self { appearance: Appearance::System, terminal: TerminalSettings::default() }
    }
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            color_scheme: "auto".to_owned(),
            font_family: String::new(),
            font_size: 13,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 5000,
        }
    }
}

impl Settings {
    fn normalize(mut self) -> Self {
        let terminal = &mut self.terminal;
        terminal.font_family = terminal.font_family.trim().to_owned();
        terminal.font_size = terminal.font_size.clamp(6, 48);
        terminal.scrollback = terminal.scrollback.min(100_000);
        if terminal.color_scheme.trim().is_empty() {
            terminal.color_scheme = "auto".to_owned();
        }
        self
    }
}

pub struct SettingsStore {
    path: PathBuf,
    settings: Mutex<Settings>,
}

impl SettingsStore {
    /// Missing or unreadable files fall back to the defaults, so a bad edit never blocks
    /// the app from starting.
    pub fn load(path: PathBuf) -> Self {
        let settings = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .unwrap_or_default()
            .normalize();
        Self { path, settings: Mutex::new(settings) }
    }

    pub fn get(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Validates, stores and returns the settings actually in effect.
    pub fn set(&self, settings: Settings) -> Result<Settings> {
        let settings = settings.normalize();
        let mut current = self.settings.lock().unwrap();
        write_json_atomic(&self.path, &settings)?;
        *current = settings.clone();
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_files_fill_in_defaults_and_values_are_clamped() {
        let settings: Settings = serde_json::from_str(r#"{"terminal":{"fontSize":200,"colorScheme":""}}"#).unwrap();
        let settings = settings.normalize();
        assert!(matches!(settings.appearance, Appearance::System));
        assert_eq!(settings.terminal.font_size, 48);
        assert_eq!(settings.terminal.color_scheme, "auto");
        assert_eq!(settings.terminal.scrollback, 5000);
    }
}
