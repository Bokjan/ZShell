//! macOS: the system's color sampler (`NSColorSampler`, macOS 10.15 and later), the magnifier
//! that the system color panel's eyedropper shows. It keeps itself alive until the user picks
//! or cancels (Escape), then calls its handler on the main thread.

use std::cell::Cell;

use block2::RcBlock;
use objc2::runtime::AnyClass;
use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};
use tauri::WebviewWindow;

use super::{Done, Rgb};
use crate::error::{Error, Result};

pub(super) fn start(window: &WebviewWindow, done: Done) -> Result<()> {
    window
        .run_on_main_thread(move || {
            // Before macOS 10.15 there is none, and naming the class would abort.
            if AnyClass::get(c"NSColorSampler").is_none() {
                let _ = done.send(Err(Error::new("colorPick.unsupported")));
                return;
            }
            let done = Cell::new(Some(done));
            let handler = RcBlock::new(move |color: *mut NSColor| {
                let Some(done) = done.take() else { return };
                // SAFETY: when not null (cancelled), `color` is a valid NSColor for the duration
                // of the block.
                let color = unsafe { color.as_ref() };
                // The sampler's color is in the screen's color space (Display P3 on most Macs);
                // the page's colors are sRGB.
                let picked = color
                    .and_then(|color| color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()))
                    .map(|color| Rgb::from_unit(color.redComponent(), color.greenComponent(), color.blueComponent()));
                let _ = done.send(Ok(picked));
            });
            let sampler = NSColorSampler::new();
            // SAFETY: on the main thread; the handler only sends on a channel.
            unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
        })
        .map_err(|e| Error::new("colorPick.failed").detail(e))
}
