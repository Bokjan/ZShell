//! The installed fonts, as far as terminal ligatures need them: the rules of a font's
//! ligatures, for the frontend to find where they form (`lib/fontLigatures`). The web view
//! can't give it the font's data (only WebView2 has `queryLocalFonts`, which gives a whole
//! collection), so the system's font APIs find the font here: DirectWrite on Windows (which
//! has the fonts installed from the Microsoft Store, in packages no font folder lists),
//! CoreText on macOS, fontconfig on Linux.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod sfnt;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
use win as platform;

use serde::Serialize;
use ts_rs::TS;

/// A font's ligature rules: the font with only the tables they are found with (see
/// [`sfnt::ligature_font`]), a few hundred KB.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LigatureFont {
    /// The family found, one of those asked for.
    pub family: String,
    /// The font file, in base64.
    pub data: String,
}

/// CSS's generic families, which stand for a font the page can't name.
const GENERIC_FAMILIES: [&str; 13] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "emoji",
    "math",
    "fangsong",
];

/// The ligature rules of the first of `families` (a font list, in order) that is installed,
/// as the web view picks the font it draws with: its regular face, whose rules the others
/// share. `None` when none is, or when a generic family comes first, or when the font is
/// one this can't read.
pub fn ligature_font(families: &[String]) -> Option<LigatureFont> {
    for family in families.iter().map(|family| family.trim()).filter(|family| !family.is_empty()) {
        if GENERIC_FAMILIES.iter().any(|generic| generic.eq_ignore_ascii_case(family)) {
            return None;
        }
        if let Some(tables) = platform::tables(family, &sfnt::TABLES) {
            let data = sfnt::ligature_font(&tables)?;
            return Some(LigatureFont { family: family.to_owned(), data: data_encoding::BASE64.encode(&data) });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font every system of the platform has.
    #[cfg(windows)]
    const INSTALLED: &str = "Segoe UI";
    #[cfg(target_os = "macos")]
    const INSTALLED: &str = "Menlo";

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn finds_the_first_installed_family() {
        let families = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        let font = ligature_font(&families(&["No Such Font ZShell", INSTALLED, "monospace"])).unwrap();
        assert_eq!(font.family, INSTALLED);
        let data = data_encoding::BASE64.decode(font.data.as_bytes()).unwrap();
        let tables = sfnt::read_tables(&mut std::io::Cursor::new(&data), 0, &sfnt::TABLES).unwrap();
        assert!(tables.iter().any(|(tag, _)| tag == b"cmap"));
        // DirectWrite matches names ignoring case, as CSS does.
        #[cfg(windows)]
        assert!(ligature_font(&families(&[&INSTALLED.to_uppercase()])).is_some());
        // Which font a generic family is isn't known.
        assert!(ligature_font(&families(&["monospace", INSTALLED])).is_none());
        assert!(ligature_font(&families(&["No Such Font ZShell"])).is_none());
    }
}
