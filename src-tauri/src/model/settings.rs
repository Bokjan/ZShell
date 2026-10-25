//! App-wide preferences (language, appearance, text size, app font, status bar, terminal,
//! tabs, sidebar, power, files, ZMODEM, session logs, updates), as saved in `settings.json` (see
//! `crate::settings`). The frontend interprets them, but for the language, which backend text
//! uses too; the backend validates them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// A language there is a catalog for, or "system" for the system's preferred languages
    /// (see `i18n.rs`). A string, so that an unknown one (from a newer version) falls back
    /// rather than making the whole file unreadable.
    pub language: String,
    pub appearance: Appearance,
    pub text_size: TextSize,
    /// The interface's font, a CSS font family list put before the default ones; empty for
    /// the default (the terminal's is `terminal.font_family`).
    pub app_font: String,
    pub status_bar: StatusBarSettings,
    pub terminal: TerminalSettings,
    pub tabs: TabSettings,
    pub sidebar: SidebarSettings,
    pub power: PowerSettings,
    pub files: FileSettings,
    pub zmodem: ZmodemSettings,
    pub logs: LogSettings,
    pub updates: UpdateSettings,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

/// The size of the interface's text (not the terminal's, which has its own font size).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TextSize {
    #[default]
    Normal,
    Large,
    Larger,
}

/// The bar under the terminals with the focused pane's connection, activity, terminal type,
/// size and cursor position.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusBarSettings {
    pub show: bool,
}

/// Checking for a newer version (`update.rs`).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    /// Once a day, automatically.
    pub check: bool,
    /// A version the user doesn't want to hear about again.
    pub skipped: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check: true, skipped: None }
    }
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self { show: true }
    }
}

/// The terminal font sizes allowed, here and in session profiles.
pub const FONT_SIZE_MIN: u16 = 6;
pub const FONT_SIZE_MAX: u16 = 48;
/// The most lines a terminal keeps above the screen.
pub const SCROLLBACK_MAX: u32 = 100_000;
/// The most a program can copy at once (OSC 52), in KiB: xterm.js drops OSC sequences past
/// 10 MB, about 7 MiB of text once decoded from base64.
pub const CLIPBOARD_LIMIT_MAX: u32 = 7168;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalSettings {
    /// A built-in color scheme id, or "auto" to follow the appearance.
    pub color_scheme: String,
    /// Preferred font family; empty uses the platform default. Fallbacks (including CJK
    /// fonts) are always appended by the frontend.
    pub font_family: String,
    pub font_size: u16,
    pub ligatures: Ligatures,
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
    /// Programs in the terminal can copy text to the clipboard (OSC 52); they can never read it.
    pub clipboard_write: bool,
    /// The most text they copy at once, in KiB; more is ignored.
    pub clipboard_limit: u32,
    /// Programs in the terminal can notify the user (OSC 9, 777 and 99) when it is out of sight.
    pub notifications: bool,
    /// Whether screen readers can read the terminal and hear new output (xterm.js's
    /// `screenReaderMode`), which slows down terminals with much output: by default while the
    /// system's screen reader runs.
    pub screen_reader_mode: ScreenReaderMode,
    /// `screenReader`, on or off, as versions before 3.4.0 saved it: read into
    /// `screen_reader_mode` by [`Settings::normalize`], off (the old default) as automatic.
    /// Saved alongside it, on for "On" only, so that those versions still read the terminal
    /// for screen readers (see [`Settings::stored`]); never sent to the frontend.
    #[serde(rename = "screenReader", skip_serializing_if = "Option::is_none")]
    #[ts(skip)]
    screen_reader: Option<bool>,
    /// A bell (BEL) not caused by typing marks its tab if in the background, tells screen
    /// readers, and asks for attention while the window is in the background.
    pub bell_alert: bool,
    /// A bell plays the system's alert sound.
    pub bell_sound: bool,
    /// A bell flashes its terminal (a visual bell).
    pub bell_flash: bool,
}

/// When terminals are read for screen readers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScreenReaderMode {
    /// While the system's screen reader runs (see `screen_reader`).
    #[default]
    Auto,
    On,
    Off,
}

/// Which character sequences the terminal draws together, so that a font with programming
/// ligatures draws them as one (the font still decides how they look).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Ligatures {
    #[default]
    Off,
    /// A fixed list of the usual ones (`->`, `!=`, `===`), whatever the font.
    Rules,
    /// Those the font's own rules make (`crate::fonts`), or the list where its rules can't be read.
    Full,
}

/// What right-clicking the terminal does.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RightClick {
    #[default]
    Menu,
    /// Paste, as in Xshell and PuTTY.
    Paste,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TabSettings {
    /// Show the title set by the shell (OSC 0 / 2) instead of the session name.
    pub follow_remote_title: bool,
    /// Ask before closing tabs that are connected or running a program.
    pub confirm_close: bool,
    /// Mark the tabs in the background that have new output, until they are shown.
    pub mark_output: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct PowerSettings {
    /// Keep the system from sleeping while idle as long as sessions are connected (see `keep_awake`).
    pub keep_awake: KeepAwakeMode,
}

/// When the system is kept from sleeping while sessions are connected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum KeepAwakeMode {
    #[default]
    Off,
    /// While the computer runs on external power, not on its battery or a UPS.
    PluggedIn,
    Always,
}

/// Remote files: SFTP and ZMODEM downloads, and editing remote files locally.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct FileSettings {
    /// Where downloads go without asking; empty for the Downloads folder.
    pub download_directory: String,
    /// The application remote files are edited with (a `.app` on macOS, an executable on
    /// Windows); empty for the one the system opens the file type with.
    pub editor: String,
}

#[derive(Clone, Debug, Default, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ZmodemSettings {
    /// What happens when `sz` sends files.
    pub receive: ZmodemReceive,
}

/// What happens when `sz` sends files. Asked by default: a file that looks like `sz` output
/// (`cat` of one) starts a download, which shouldn't happen without the user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ZmodemReceive {
    #[default]
    Ask,
    /// Saved into the download folder.
    Downloads,
    /// A folder is chosen each time.
    ChooseFolder,
}

/// [`ZmodemSettings`] as saved, also by versions before 1.6.12, which only had
/// `askDownloadLocation` (saving into Downloads without asking unless it was set).
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct StoredZmodemSettings {
    receive: Option<ZmodemReceive>,
    ask_download_location: bool,
}

impl<'de> Deserialize<'de> for ZmodemSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let stored = StoredZmodemSettings::deserialize(deserializer)?;
        let receive = stored.receive.unwrap_or(match stored.ask_download_location {
            true => ZmodemReceive::ChooseFolder,
            false => ZmodemReceive::Ask,
        });
        Ok(Self { receive })
    }
}

/// Session logs (see `logging/`).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct LogSettings {
    /// Where logs are written; empty for the default (`ZShellLogs` in Documents).
    pub directory: String,
    /// File name, with `{session}`, `{host}`, `{user}`, `{date}` and `{time}` replaced.
    pub file_name: String,
    pub format: LogFormat,
    /// Start each line with the time it was written (plain text only).
    pub timestamps: bool,
    /// How that time is written, as are the times of our own messages in the terminal (see
    /// `logging/time_format.rs`).
    pub time_format: String,
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
            time_format: crate::logging::time_format::DEFAULT_TIME_FORMAT.to_owned(),
            auto_local: false,
            keep_days: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
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
            language: crate::i18n::SYSTEM.to_owned(),
            appearance: Appearance::System,
            text_size: TextSize::Normal,
            app_font: String::new(),
            status_bar: StatusBarSettings::default(),
            terminal: TerminalSettings::default(),
            tabs: TabSettings::default(),
            sidebar: SidebarSettings::default(),
            power: PowerSettings::default(),
            files: FileSettings::default(),
            zmodem: ZmodemSettings::default(),
            logs: LogSettings::default(),
            updates: UpdateSettings::default(),
        }
    }
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            color_scheme: "auto".to_owned(),
            font_family: String::new(),
            font_size: 13,
            ligatures: Ligatures::Off,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 5000,
            copy_on_select: false,
            right_click: RightClick::Menu,
            confirm_multiline_paste: true,
            option_as_meta: false,
            clipboard_write: true,
            clipboard_limit: 1024,
            notifications: true,
            screen_reader_mode: ScreenReaderMode::Auto,
            screen_reader: None,
            bell_alert: true,
            bell_sound: true,
            bell_flash: false,
        }
    }
}

impl Default for TabSettings {
    fn default() -> Self {
        Self { follow_remote_title: true, confirm_close: true, mark_output: true }
    }
}

impl Settings {
    pub(crate) fn normalize(mut self) -> Self {
        if !crate::i18n::is_setting(&self.language) {
            self.language = crate::i18n::SYSTEM.to_owned();
        }
        self.app_font = self.app_font.trim().to_owned();
        let terminal = &mut self.terminal;
        terminal.font_family = terminal.font_family.trim().to_owned();
        terminal.font_size = terminal.font_size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        terminal.scrollback = terminal.scrollback.min(SCROLLBACK_MAX);
        terminal.clipboard_limit = terminal.clipboard_limit.clamp(1, CLIPBOARD_LIMIT_MAX);
        if terminal.color_scheme.trim().is_empty() {
            terminal.color_scheme = "auto".to_owned();
        }
        if terminal.screen_reader.take() == Some(true) {
            terminal.screen_reader_mode = ScreenReaderMode::On;
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
        logs.time_format = logs.time_format.trim().to_owned();
        if logs.time_format.is_empty() || crate::logging::TimeFormat::parse(&logs.time_format).is_err() {
            logs.time_format = crate::logging::time_format::DEFAULT_TIME_FORMAT.to_owned();
        }
        self
    }

    /// The settings as written to the file, with the keys versions before 3.4.0 read. Their
    /// `screenReader` agrees with `screenReaderMode`, so this version reads it back unchanged;
    /// they drop `screenReaderMode` when they save, and then their value is the one read.
    pub(crate) fn stored(&self) -> Self {
        let mut stored = self.clone();
        stored.terminal.screen_reader = Some(stored.terminal.screen_reader_mode == ScreenReaderMode::On);
        stored
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
        assert!(matches!(settings.text_size, TextSize::Normal));
        assert_eq!(settings.terminal.font_size, 48);
        assert_eq!(settings.terminal.color_scheme, "auto");
        assert_eq!(settings.terminal.scrollback, 5000);
        assert_eq!(settings.terminal.ligatures, Ligatures::Off);
        assert!(settings.terminal.clipboard_write && settings.terminal.notifications);
        assert_eq!(settings.terminal.clipboard_limit, 1024);
        assert!(settings.terminal.confirm_multiline_paste && settings.tabs.confirm_close);
        assert!(settings.status_bar.show);
        assert_eq!(settings.power.keep_awake, KeepAwakeMode::Off);
        assert_eq!(settings.language, "system");
    }

    #[test]
    fn time_formats_that_cannot_be_used_fall_back_to_the_default() {
        let format = |json: &str| serde_json::from_str::<Settings>(json).unwrap().normalize().logs.time_format;
        assert_eq!(format("{}"), "[%Y-%m-%d %H:%M:%S]");
        assert_eq!(format(r#"{"logs":{"timeFormat":" %H:%M:%S.%3f "}}"#), "%H:%M:%S.%3f");
        assert_eq!(format(r#"{"logs":{"timeFormat":"%A"}}"#), "[%Y-%m-%d %H:%M:%S]");
        assert_eq!(format(r#"{"logs":{"timeFormat":"  "}}"#), "[%Y-%m-%d %H:%M:%S]");
    }

    #[test]
    fn unknown_languages_follow_the_system() {
        let language = |json: &str| serde_json::from_str::<Settings>(json).unwrap().normalize().language;
        assert_eq!(language(r#"{"language":"en"}"#), "en");
        assert_eq!(language(r#"{"language":"xx"}"#), "system");
    }

    #[test]
    fn reads_the_screen_reader_setting_of_earlier_versions() {
        let mode = |json: &str| serde_json::from_str::<Settings>(json).unwrap().normalize().terminal.screen_reader_mode;
        assert_eq!(mode("{}"), ScreenReaderMode::Auto);
        assert_eq!(mode(r#"{"terminal":{"screenReader":true}}"#), ScreenReaderMode::On);
        // Off was the default: now automatic.
        assert_eq!(mode(r#"{"terminal":{"screenReader":false}}"#), ScreenReaderMode::Auto);
        assert_eq!(mode(r#"{"terminal":{"screenReaderMode":"off"}}"#), ScreenReaderMode::Off);
        // Sent to the frontend under the new key only.
        let sent = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(sent["terminal"]["screenReaderMode"], "auto");
        assert!(sent["terminal"].get("screenReader").is_none());
        // Saved with the old key too, for earlier versions, which reads back the same.
        for (mode, on) in [(ScreenReaderMode::Auto, false), (ScreenReaderMode::On, true), (ScreenReaderMode::Off, false)] {
            let mut settings = Settings::default();
            settings.terminal.screen_reader_mode = mode;
            let saved = serde_json::to_value(settings.stored()).unwrap();
            assert_eq!(saved["terminal"]["screenReader"], on);
            let read = serde_json::from_value::<Settings>(saved).unwrap().normalize();
            assert_eq!(read.terminal.screen_reader_mode, mode);
            assert_eq!(read.terminal.screen_reader, None);
        }
    }

    #[test]
    fn reads_the_zmodem_setting_of_earlier_versions() {
        let receive = |json: &str| serde_json::from_str::<Settings>(json).unwrap().zmodem.receive;
        assert_eq!(receive("{}"), ZmodemReceive::Ask);
        // Saved without asking before; now asked, the new default.
        assert_eq!(receive(r#"{"zmodem":{"askDownloadLocation":false}}"#), ZmodemReceive::Ask);
        assert_eq!(receive(r#"{"zmodem":{"askDownloadLocation":true}}"#), ZmodemReceive::ChooseFolder);
        assert_eq!(receive(r#"{"zmodem":{"receive":"downloads"}}"#), ZmodemReceive::Downloads);
        let saved = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(saved["zmodem"], serde_json::json!({ "receive": "ask" }));
    }
}
