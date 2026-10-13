//! Message catalog for text produced by the backend: terminal prompts and error messages.
//!
//! Catalogs are JSON files in `src-tauri/locales/`, embedded at compile time. Keys are dotted
//! paths into the JSON object and values use `{name}` placeholders, the same syntax as the
//! frontend catalogs. English is the source language and the fallback for missing keys.

use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::sync::{LazyLock, RwLock};

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

pub const FALLBACK: &str = "en";

/// The `language` setting that follows the system's preferred languages.
pub const SYSTEM: &str = "system";

/// The catalogs by language tag, English first. The frontend has one for each (`src/locales`).
const SOURCES: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en.json")),
    ("zh-Hans", include_str!("../locales/zh-Hans.json")),
    ("ja", include_str!("../locales/ja.json")),
    ("de", include_str!("../locales/de.json")),
    ("fr", include_str!("../locales/fr.json")),
];

type Catalog = HashMap<String, String>;

static CATALOGS: LazyLock<HashMap<&'static str, Catalog>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(locale, json)| {
            let value: Value = serde_json::from_str(json).unwrap_or_else(|e| panic!("invalid {locale} catalog: {e}"));
            let mut catalog = Catalog::new();
            flatten("", &value, &mut catalog);
            (*locale, catalog)
        })
        .collect()
});

static LOCALE: RwLock<&str> = RwLock::new(FALLBACK);

/// The system's preferred languages, read once: a change applies when ZShell starts again.
static SYSTEM_LOCALES: LazyLock<Vec<String>> = LazyLock::new(|| sys_locale::get_locales().collect());

fn flatten(prefix: &str, value: &Value, out: &mut Catalog) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(&path, child, out);
            }
        }
        Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        _ => {}
    }
}

/// The languages there are catalogs for, English first, for the bindings.
#[cfg(test)]
pub fn languages() -> Vec<&'static str> {
    SOURCES.iter().map(|(locale, _)| *locale).collect()
}

/// Whether `language` is a value the `language` setting can have.
pub fn is_setting(language: &str) -> bool {
    language == SYSTEM || SOURCES.iter().any(|(locale, _)| *locale == language)
}

/// The languages in effect, for the frontend.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Locales {
    /// The language of the interface.
    pub current: String,
    /// What the `system` setting chooses.
    pub system: String,
    /// The first CJK language among the system's preferred ones, whose fonts come first when
    /// the interface isn't in one: the same code point has different glyphs in Chinese and
    /// Japanese fonts.
    pub cjk: Cjk,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
pub enum Cjk {
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "zh-Hant")]
    ZhHant,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
}

/// Switches to the language the `language` setting chooses and returns it.
pub fn apply(setting: &str) -> &'static str {
    let locale = choose(setting, &SYSTEM_LOCALES);
    *LOCALE.write().unwrap() = locale;
    locale
}

/// The languages in effect.
pub fn locales() -> Locales {
    let current = *LOCALE.read().unwrap();
    Locales { current: current.to_owned(), system: negotiate(&SYSTEM_LOCALES).to_owned(), cjk: cjk(&SYSTEM_LOCALES) }
}

/// The language `setting` chooses, with `preferred` the system's languages.
fn choose(setting: &str, preferred: &[String]) -> &'static str {
    match SOURCES.iter().find(|(locale, _)| *locale == setting) {
        Some((locale, _)) => locale,
        None => negotiate(preferred),
    }
}

/// The first of the `preferred` languages there is a catalog for, or English.
fn negotiate(preferred: &[String]) -> &'static str {
    preferred.iter().find_map(|tag| lookup(tag)).unwrap_or(FALLBACK)
}

/// The catalog for a BCP 47 tag: "de-AT" tries "de-AT", then "de". Chinese goes by script
/// rather than region (see [`normalize`]), so Traditional Chinese finds none.
fn lookup(tag: &str) -> Option<&'static str> {
    let mut tag = normalize(tag);
    loop {
        if let Some((locale, _)) = SOURCES.iter().find(|(locale, _)| locale.eq_ignore_ascii_case(&tag)) {
            return Some(locale);
        }
        tag.truncate(tag.rfind('-')?);
    }
}

/// A tag as BCP 47 (POSIX's "zh_CN.UTF-8" too), with Chinese as its script alone: "zh-CN" and
/// "zh-Hans-SG" are "zh-Hans", "zh-TW", "zh-HK", "zh-MO" and "zh-Hant-TW" are "zh-Hant".
fn normalize(tag: &str) -> String {
    let tag = tag.split(['.', '@']).next().unwrap_or_default().replace('_', "-");
    let mut parts = tag.split('-');
    if !parts.next().is_some_and(|language| language.eq_ignore_ascii_case("zh")) {
        return tag;
    }
    let traditional = parts.any(|part| ["hant", "tw", "hk", "mo"].iter().any(|t| part.eq_ignore_ascii_case(t)));
    (if traditional { "zh-Hant" } else { "zh-Hans" }).to_owned()
}

/// The first CJK language among the `preferred` ones (see [`Locales::cjk`]); Simplified
/// Chinese without one.
fn cjk(preferred: &[String]) -> Cjk {
    preferred
        .iter()
        .find_map(|tag| {
            let tag = normalize(tag);
            match tag.split('-').next().unwrap_or_default().to_ascii_lowercase().as_str() {
                "zh" if tag == "zh-Hant" => Some(Cjk::ZhHant),
                "zh" => Some(Cjk::ZhHans),
                "ja" => Some(Cjk::Ja),
                "ko" => Some(Cjk::Ko),
                _ => None,
            }
        })
        .unwrap_or(Cjk::ZhHans)
}

/// Looks up `key` in the current locale (falling back to English, then to the key itself)
/// and fills in `{name}` placeholders. Prefer the `t!` macro.
pub fn translate(key: &str, args: &[(&str, &dyn Display)]) -> String {
    let locale = *LOCALE.read().unwrap();
    let Some(template) = CATALOGS[locale].get(key).or_else(|| CATALOGS[FALLBACK].get(key)) else {
        return key.to_owned();
    };
    interpolate(template, args)
}

fn interpolate(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let value = after.find('}').and_then(|end| args.iter().find(|(name, _)| *name == &after[..end]).map(|(_, value)| (end, value)));
        match value {
            Some((end, value)) => {
                let _ = write!(out, "{value}");
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `t!("terminal.connecting", user = name, port = 22)` → the translated, interpolated string.
/// Available crate-wide because `lib.rs` declares this module first with `#[macro_use]`.
macro_rules! t {
    ($key:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::translate($key, &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|tag| tag.to_string()).collect()
    }

    #[test]
    fn normalizes_tags() {
        assert_eq!(normalize("en_GB.UTF-8"), "en-GB");
        assert_eq!(normalize("de_DE@euro"), "de-DE");
        assert_eq!(normalize("zh-Hans-CN"), "zh-Hans");
        assert_eq!(normalize("zh_CN.UTF-8"), "zh-Hans");
        assert_eq!(normalize("zh-SG"), "zh-Hans");
        assert_eq!(normalize("zh"), "zh-Hans");
        assert_eq!(normalize("zh-TW"), "zh-Hant");
        assert_eq!(normalize("zh-Hant-HK"), "zh-Hant");
        assert_eq!(normalize("zh_HK"), "zh-Hant");
    }

    #[test]
    fn negotiates_the_first_supported_language() {
        assert_eq!(negotiate(&tags(&["en-US", "ja-JP"])), "en");
        assert_eq!(negotiate(&tags(&["xx-YY", "en_GB"])), "en");
        assert_eq!(negotiate(&tags(&["xx-YY", "C"])), FALLBACK);
        assert_eq!(negotiate(&[]), FALLBACK);
        for (locale, _) in SOURCES {
            assert_eq!(negotiate(&tags(&["xx", locale])), *locale);
        }
    }

    #[test]
    fn chooses_the_setting_or_the_system_language() {
        let system = tags(&["xx-YY"]);
        assert_eq!(choose("en", &system), "en");
        assert_eq!(choose(SYSTEM, &system), FALLBACK);
        // Unknown values (a newer version's language) follow the system.
        assert_eq!(choose("xx", &system), FALLBACK);
    }

    #[test]
    fn finds_the_first_cjk_language() {
        assert_eq!(cjk(&tags(&["en-US", "ja-JP", "zh-Hans-CN"])), Cjk::Ja);
        assert_eq!(cjk(&tags(&["de-DE", "zh-TW"])), Cjk::ZhHant);
        assert_eq!(cjk(&tags(&["ko-KR"])), Cjk::Ko);
        assert_eq!(cjk(&tags(&["zh_CN.UTF-8"])), Cjk::ZhHans);
        assert_eq!(cjk(&tags(&["en-US"])), Cjk::ZhHans);
    }

    #[test]
    fn interpolates_named_placeholders() {
        let args: [(&str, &dyn Display); 2] = [("name", &"world"), ("n", &3)];
        assert_eq!(interpolate("hello {name}, {n} {unknown} {", &args), "hello world, 3 {unknown} {");
    }

    /// The `{name}` placeholders of a string.
    fn placeholders(text: &str) -> std::collections::BTreeSet<&str> {
        text.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(name, _)| name)).filter(|name| !name.is_empty()).collect()
    }

    /// Every catalog has the keys of the English one, translated, with the same placeholders.
    #[test]
    fn catalogs_are_complete() {
        let en = &CATALOGS[FALLBACK];
        let mut wrong = Vec::new();
        for (locale, catalog) in CATALOGS.iter().filter(|(locale, _)| **locale != FALLBACK) {
            for key in en.keys().filter(|key| !catalog.contains_key(*key)) {
                wrong.push(format!("{locale}: {key} is missing"));
            }
            for (key, text) in catalog {
                match en.get(key) {
                    None => wrong.push(format!("{locale}: {key} is not in en.json")),
                    Some(_) if text.trim().is_empty() => wrong.push(format!("{locale}: {key} is empty")),
                    Some(source) if placeholders(source) != placeholders(text) => {
                        wrong.push(format!("{locale}: {key} has {:?} instead of {:?}", placeholders(text), placeholders(source)))
                    }
                    Some(_) => {}
                }
            }
        }
        wrong.sort();
        assert!(wrong.is_empty(), "catalogs differing from locales/en.json:\n{}", wrong.join("\n"));
    }

    /// The files outside the catalogs that list the languages: macOS's localizations, and the
    /// polkit action's text (POSIX names: polkit drops the region only after trying it, and
    /// Chinese needs one to tell the script).
    #[test]
    fn packages_declare_every_language() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let plist = std::fs::read_to_string(root.join("Info.plist")).unwrap();
        let policy = std::fs::read_to_string(root.join("linux/org.boyin.zshell.policy")).unwrap();
        for (locale, _) in SOURCES {
            assert!(plist.contains(&format!("<string>{locale}</string>")), "Info.plist lacks {locale}");
            if *locale != FALLBACK {
                let posix = if *locale == "zh-Hans" { "zh_CN" } else { locale };
                for element in ["description", "message"] {
                    let tag = format!("<{element} xml:lang=\"{posix}\">");
                    assert!(policy.contains(&tag), "the polkit policy lacks {tag}");
                }
            }
        }
    }

    /// The sources, with their paths.
    fn sources() -> Vec<(std::path::PathBuf, String)> {
        let mut sources = Vec::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    let source = std::fs::read_to_string(&path).unwrap();
                    sources.push((path, source));
                }
            }
        }
        sources
    }

    /// Every `t!("…")` key and `Error::new("…")` code used in the sources must exist in the
    /// English catalog.
    #[test]
    fn catalog_covers_all_keys_in_sources() {
        let en = &CATALOGS[FALLBACK];
        let mut missing = Vec::new();
        for (path, source) in sources() {
            // `errors.error(...)`: the proxy handshakes' helper (`proxy::Errors`).
            for (marker, prefix) in [("t!(\"", ""), ("Error::new(\"", "errors."), ("errors.error(\"", "errors.")] {
                for (at, _) in source.match_indices(marker) {
                    // Skip `format!(` and friends: the marker must start a token.
                    let preceding = source[..at].chars().next_back();
                    if preceding.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = &source[at + marker.len()..];
                    let name = &rest[..rest.find('"').unwrap()];
                    let is_key = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
                    let key = format!("{prefix}{name}");
                    if is_key && !en.contains_key(&key) {
                        missing.push(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "keys missing from locales/en.json:\n{}", missing.join("\n"));
    }

    /// Every key in the English catalog must be used, so that unused ones are removed. A key
    /// counts as used when it appears as a string literal, since some are chosen before being
    /// passed to `t!` (an error code, as such, without its `errors.` prefix).
    #[test]
    fn catalog_has_no_unused_keys() {
        let sources: String = sources().into_iter().map(|(_, source)| source).collect();
        let mut unused: Vec<_> = CATALOGS[FALLBACK]
            .keys()
            .filter(|key| {
                let code = key.strip_prefix("errors.");
                !sources.contains(&format!("\"{key}\"")) && !code.is_some_and(|code| sources.contains(&format!("\"{code}\"")))
            })
            .cloned()
            .collect();
        unused.sort();
        assert!(unused.is_empty(), "keys in locales/en.json that no source uses:\n{}", unused.join("\n"));
    }
}
