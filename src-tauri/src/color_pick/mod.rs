//! Picking a color from anywhere on the screen, for the color picker's eyedropper.
//!
//! The web views can't: Chromium's `EyeDropper` (and the eyedropper of the color popup that
//! `<input type="color">` opens) needs the embedder to implement it, which WebView2 doesn't
//! (`open()` rejects at once, as if the user had cancelled), and WebKit has no `EyeDropper`.
//! So each platform picks natively: Windows with a window of its own over a screenshot,
//! macOS with the system's color sampler (`NSColorSampler`), Linux with the screenshot portal
//! (`org.freedesktop.portal.Screenshot.PickColor`).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(windows)]
use win as platform;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::WebviewWindow;
use tokio::sync::oneshot;

use crate::error::{Error, Result};

/// A color in sRGB.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rgb(u8, u8, u8);

impl Rgb {
    /// From components between 0 and 1, as macOS and the portal give them; out of range ones
    /// (an extended sRGB color, from an HDR screen) are clamped.
    #[cfg_attr(windows, allow(dead_code))]
    fn from_unit(red: f64, green: f64, blue: f64) -> Self {
        let byte = |unit: f64| (unit.clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgb(byte(red), byte(green), byte(blue))
    }

    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Where a platform sends what the user picked: `None` if they cancelled.
type Done = oneshot::Sender<Result<Option<Rgb>>>;

/// Whether a pick is under way: one at a time.
static PICKING: AtomicBool = AtomicBool::new(false);

/// Lets the user pick a color from the screen, from `window`; `#rrggbb`, or `None` if they
/// cancelled. Waits until they have picked.
pub async fn pick(window: WebviewWindow) -> Result<Option<String>> {
    if PICKING.swap(true, Ordering::AcqRel) {
        return Err(Error::new("colorPick.busy"));
    }
    struct Picking;
    impl Drop for Picking {
        fn drop(&mut self) {
            PICKING.store(false, Ordering::Release);
        }
    }
    let _picking = Picking;
    let (done, picked) = oneshot::channel();
    platform::start(&window, done)?;
    let picked = picked.await.map_err(|_| Error::new("colorPick.failed"))??;
    Ok(picked.map(Rgb::hex))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_colors_in_hex() {
        assert_eq!(Rgb(0x1e, 0x00, 0xff).hex(), "#1e00ff");
        assert_eq!(Rgb::from_unit(1.0, 0.5, 0.0).hex(), "#ff8000");
        assert_eq!(Rgb::from_unit(1.2, -0.1, 0.2).hex(), "#ff0033");
    }
}
