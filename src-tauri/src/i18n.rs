//! Message catalog for text produced by the backend: terminal prompts and error messages.
//!
//! Catalogs are JSON files in `src-tauri/locales/`, embedded at compile time. Keys are dotted
//! paths into the JSON object and values use `{name}` placeholders, the same syntax as the
//! frontend catalogs. English is the source language and the fallback for missing keys.

use std::collections::HashMap;
use std::fmt::{Display, Write};
use std::sync::{LazyLock, RwLock};

use serde_json::Value;

pub const FALLBACK: &str = "en";

const SOURCES: &[(&str, &str)] = &[("en", include_str!("../locales/en.json"))];

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

/// Switches to the best supported match for a BCP 47 tag and returns the chosen locale.
pub fn set_locale(requested: &str) -> &'static str {
    let resolved = resolve(requested);
    *LOCALE.write().unwrap() = resolved;
    resolved
}

/// "zh-Hans-CN" tries "zh-Hans-CN", "zh-Hans", "zh", then falls back to English.
fn resolve(requested: &str) -> &'static str {
    let mut tag = requested.replace('_', "-");
    loop {
        if let Some((locale, _)) = SOURCES.iter().find(|(locale, _)| locale.eq_ignore_ascii_case(&tag)) {
            return locale;
        }
        match tag.rfind('-') {
            Some(i) => tag.truncate(i),
            None => return FALLBACK,
        }
    }
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
        let value = after
            .find('}')
            .and_then(|end| args.iter().find(|(name, _)| *name == &after[..end]).map(|(_, value)| (end, value)));
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

    #[test]
    fn resolves_locale_tags() {
        assert_eq!(resolve("en-US"), "en");
        assert_eq!(resolve("en_GB"), "en");
        assert_eq!(resolve("xx-YY"), FALLBACK);
    }

    #[test]
    fn interpolates_named_placeholders() {
        let args: [(&str, &dyn Display); 2] = [("name", &"world"), ("n", &3)];
        assert_eq!(interpolate("hello {name}, {n} {unknown} {", &args), "hello world, 3 {unknown} {");
    }

    /// Every `t!("…")` key and `Error::new("…")` code used in the sources must exist in the
    /// English catalog.
    #[test]
    fn catalog_covers_all_keys_in_sources() {
        let en = &CATALOGS[FALLBACK];
        let mut missing = Vec::new();
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut dirs = vec![src];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                for (marker, prefix) in [("t!(\"", ""), ("Error::new(\"", "errors.")] {
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
        }
        assert!(missing.is_empty(), "keys missing from locales/en.json:\n{}", missing.join("\n"));
    }
}
