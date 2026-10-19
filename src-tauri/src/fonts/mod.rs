//! The installed fonts, as far as terminal ligatures need them: the substitutions of a font,
//! for the frontend to find where ligatures form (`lib/fontLigatures.ts`). The web view can't
//! give it the font's data (only WebView2 has `queryLocalFonts`, which gives a whole
//! collection), so the system's font APIs find the font here: DirectWrite on Windows (which
//! has the fonts installed from the Microsoft Store, in packages no font folder lists),
//! CoreText on macOS, fontconfig on Linux. Its `cmap`, `GDEF` and `GSUB` are read here
//! (`cmap.rs`, `gsub.rs`) into the compact rules the frontend applies (`rules.rs`).

mod cmap;
mod gsub;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod read;
pub mod rules;
mod sfnt;
#[cfg(test)]
mod testing;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
use win as platform;

pub use rules::LigatureRules;

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

/// The substitutions of the first of `families` (a font list, in order) that is installed,
/// as the web view picks the font it draws with: its regular face, whose rules the others
/// share. `None` when none is, or when a generic family comes first, or when the font has no
/// characters this can read.
pub fn ligature_rules(families: &[String]) -> Option<LigatureRules> {
    for family in families.iter().map(|family| family.trim()).filter(|family| !family.is_empty()) {
        if GENERIC_FAMILIES.iter().any(|generic| generic.eq_ignore_ascii_case(family)) {
            return None;
        }
        if let Some(tables) = platform::tables(family, &sfnt::TABLES) {
            return rules_of(family, &tables);
        }
    }
    None
}

/// The rules of a font with `tables` (those of [`sfnt::TABLES`] it has).
fn rules_of(family: &str, tables: &[(sfnt::Tag, Vec<u8>)]) -> Option<LigatureRules> {
    let table = |tag: &sfnt::Tag| tables.iter().find(|(t, _)| t == tag).map_or(&[][..], |(_, data)| data.as_slice());
    let chars = cmap::mappings(table(b"cmap"));
    if chars.is_empty() {
        return None;
    }
    Some(rules::compile(family, &chars, &gsub::substitutions(table(b"GSUB")), &gsub::glyph_classes(table(b"GDEF"))))
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
        let rules = ligature_rules(&families(&["No Such Font ZShell", INSTALLED, "monospace"])).unwrap();
        assert_eq!(rules.family, INSTALLED);
        assert!(!rules.chars.is_empty());
        // DirectWrite matches names ignoring case, as CSS does.
        #[cfg(windows)]
        assert!(ligature_rules(&families(&[&INSTALLED.to_uppercase()])).is_some());
        // Which font a generic family is isn't known.
        assert!(ligature_rules(&families(&["monospace", INSTALLED])).is_none());
        assert!(ligature_rules(&families(&["No Such Font ZShell"])).is_none());
    }
}
