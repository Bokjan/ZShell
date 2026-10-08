//! App-wide preferences (appearance, terminal, tabs, sidebar, files, ZMODEM, session logs), persisted as `settings.json` next to
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
    pub tabs: TabSettings,
    pub sidebar: SidebarSettings,
    pub files: FileSettings,
    pub zmodem: ZmodemSettings,
    pub logs: LogSettings,
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
    /// Copy text to the clipboard as soon as it is selected.
    pub copy_on_select: bool,
    pub right_click: RightClick,
    /// Ask before pasting text with line breaks while the shell would run each line.
    pub confirm_multiline_paste: bool,
    /// macOS: the Option key sends Meta (Esc-prefixed) sequences instead of special characters.
    pub option_as_meta: bool,
}

/// What right-clicking the terminal does.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RightClick {
    #[default]
    Menu,
    /// Paste, as in Xshell and PuTTY.
    Paste,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TabSettings {
    /// Show the title set by the shell (OSC 0 / 2) instead of the session name.
    pub follow_remote_title: bool,
    /// Ask before closing tabs that are connected or running a program.
    pub confirm_close: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SidebarSettings {
    /// Show the most recently opened sessions above the list.
    pub show_recent: bool,
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self { show_recent: true }
    }
}

/// Remote files: SFTP and ZMODEM downloads, and editing remote files locally.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileSettings {
    /// Where downloads go without asking; empty for the Downloads folder.
    pub download_directory: String,
    /// The application remote files are edited with (a `.app` on macOS, an executable on
    /// Windows); empty for the one the system opens the file type with.
    pub editor: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ZmodemSettings {
    /// Ask for a folder when `sz` sends files, instead of saving into Downloads.
    pub ask_download_location: bool,
}

/// Session logs (see `logging.rs`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogSettings {
    /// Where logs are written; empty for the default (`ZShellLogs` in Documents).
    pub directory: String,
    /// File name, with `{session}`, `{host}`, `{user}`, `{date}` and `{time}` replaced.
    pub file_name: String,
    pub format: LogFormat,
    /// Start each line with the time it was written (plain text only).
    pub timestamps: bool,
    /// Record local terminals from the start, as sessions can be set to.
    pub auto_local: bool,
    /// Delete logs older than this many days; 0 keeps them.
    pub keep_days: u32,
}

pub const DEFAULT_LOG_FILE_NAME: &str = "{session}_{date}_{time}.log";

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            directory: String::new(),
            file_name: DEFAULT_LOG_FILE_NAME.to_owned(),
            format: LogFormat::Text,
            timestamps: false,
            auto_local: false,
            keep_days: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogFormat {
    /// Plain text: control sequences (colors, cursor movement) removed.
    #[default]
    Text,
    /// What the terminal received, control sequences included (`less -R` shows the colors).
    Raw,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            terminal: TerminalSettings::default(),
            tabs: TabSettings::default(),
            sidebar: SidebarSettings::default(),
            files: FileSettings::default(),
            zmodem: ZmodemSettings::default(),
            logs: LogSettings::default(),
        }
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
            copy_on_select: false,
            right_click: RightClick::Menu,
            confirm_multiline_paste: true,
            option_as_meta: false,
        }
    }
}

impl Default for TabSettings {
    fn default() -> Self {
        Self { follow_remote_title: true, confirm_close: true }
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
        let files = &mut self.files;
        files.download_directory = files.download_directory.trim().to_owned();
        files.editor = files.editor.trim().to_owned();
        let logs = &mut self.logs;
        logs.directory = logs.directory.trim().to_owned();
        logs.file_name = logs.file_name.trim().to_owned();
        if logs.file_name.is_empty() {
            logs.file_name = DEFAULT_LOG_FILE_NAME.to_owned();
        }
        logs.keep_days = logs.keep_days.min(3650);
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
        assert!(settings.terminal.confirm_multiline_paste && settings.tabs.confirm_close);
    }
}
