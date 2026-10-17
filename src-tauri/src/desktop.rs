//! The desktop's settings on Linux, read from the settings portal (`org.freedesktop.portal.Settings`)
//! as GTK 4 and libadwaita apps read them: the color scheme, and GNOME's monospace font.

use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use zbus::zvariant::{OwnedValue, Value};

/// How long reading a setting may take: the portal starts on demand.
const PORTAL_TIMEOUT: Duration = Duration::from_secs(2);

/// A setting's value, or `None` without a portal or the setting. Blocks for a moment at most.
fn read(namespace: &str, key: &str) -> Option<OwnedValue> {
    let connection = zbus::blocking::connection::Builder::session().ok()?.method_timeout(PORTAL_TIMEOUT).build().ok()?;
    // `Read`, which every version of the portal has, wraps the value in a second variant.
    let reply = connection
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "Read",
            &(namespace, key),
        )
        .ok()?;
    let mut value: OwnedValue = reply.body().deserialize().ok()?;
    while let Value::Value(inner) = &*value {
        value = OwnedValue::try_from(&**inner).ok()?;
    }
    Some(value)
}

/// Whether the desktop prefers dark (`org.freedesktop.appearance`): `color-scheme` is 0 for no
/// preference, 1 to prefer dark, 2 to prefer light.
pub fn prefers_dark() -> bool {
    read("org.freedesktop.appearance", "color-scheme").and_then(|value| u32::try_from(value).ok()) == Some(1)
}

/// The monospace font being read as the app starts (see [`read_monospace_font_ahead`]).
static AHEAD: Mutex<Option<JoinHandle<Option<String>>>> = Mutex::new(None);

/// Starts reading the monospace font as the app starts: the first window waits for it before
/// it shows anything, and the portal starts on demand.
pub fn read_monospace_font_ahead() {
    *AHEAD.lock().unwrap() = Some(std::thread::spawn(monospace_font));
}

/// [`monospace_font`]: read ahead for the first window, then read again for each, as it may
/// have changed.
pub fn current_monospace_font() -> Option<String> {
    let ahead = AHEAD.lock().unwrap().take();
    match ahead {
        Some(reading) => reading.join().ok().flatten(),
        None => monospace_font(),
    }
}

/// The font families of GNOME's monospace font (`Ubuntu Sans Mono 11` on Ubuntu), without its
/// size and style; `None` elsewhere.
pub fn monospace_font() -> Option<String> {
    let value = read("org.gnome.desktop.interface", "monospace-font-name")?;
    families(&String::try_from(value).ok()?)
}

/// The families of a Pango font description (`FAMILY-LIST [STYLE-OPTIONS] [SIZE]`).
fn families(description: &str) -> Option<String> {
    const STYLES: &[&str] = &[
        "normal",
        "regular",
        "book",
        "roman",
        "italic",
        "oblique",
        "small-caps",
        "thin",
        "ultra-light",
        "extra-light",
        "light",
        "semi-light",
        "medium",
        "semi-bold",
        "demi-bold",
        "bold",
        "ultra-bold",
        "extra-bold",
        "heavy",
        "black",
        "condensed",
        "semi-condensed",
        "expanded",
        "semi-expanded",
    ];
    let mut words: Vec<&str> = description.split_whitespace().collect();
    if words.last().is_some_and(|size| size.trim_end_matches("px").parse::<f64>().is_ok()) {
        words.pop();
    }
    while words.len() > 1 && words.last().is_some_and(|word| STYLES.contains(&word.to_ascii_lowercase().as_str())) {
        words.pop();
    }
    let families = words.join(" ");
    let families = families.trim_matches([' ', ',']);
    (!families.is_empty()).then(|| families.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_families_of_a_font_description() {
        assert_eq!(families("Ubuntu Sans Mono 11").as_deref(), Some("Ubuntu Sans Mono"));
        assert_eq!(families("Monospace Bold Italic 10").as_deref(), Some("Monospace"));
        assert_eq!(families("Adwaita Mono 11").as_deref(), Some("Adwaita Mono"));
        assert_eq!(families("Source Code Pro, Noto Mono 12px").as_deref(), Some("Source Code Pro, Noto Mono"));
        assert_eq!(families("DejaVu Sans Mono").as_deref(), Some("DejaVu Sans Mono"));
        assert_eq!(families("Bold").as_deref(), Some("Bold"));
        assert_eq!(families(" 11"), None);
    }
}
