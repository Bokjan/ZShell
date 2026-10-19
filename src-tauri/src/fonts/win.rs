//! Windows: a family's tables from DirectWrite's system font collection, which has the fonts
//! installed for all users, for the current user, and from the Microsoft Store.

use std::ffi::c_void;

use windows::core::{BOOL, HSTRING};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL,
};

use super::sfnt::Tag;
use super::Found;

/// The tables among `tags` of `family`'s regular face (or the nearest one), if the family is
/// installed under that name (in any language), as Chromium finds it; `None` if DirectWrite fails.
pub fn tables(family: &str, tags: &[Tag]) -> Option<Found> {
    // SAFETY: plain COM calls; the out pointers are valid for each call.
    let face = unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let mut collection = None;
        factory.GetSystemFontCollection(&mut collection, false).ok()?;
        let collection = collection?;
        let (mut index, mut exists) = (0, BOOL(0));
        collection.FindFamilyName(&HSTRING::from(family), &mut index, &mut exists).ok()?;
        if !exists.as_bool() {
            return Some(Found::Missing);
        }
        let font = collection
            .GetFontFamily(index)
            .ok()?
            .GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)
            .ok()?;
        font.CreateFontFace().ok()?
    };
    let mut tables = Vec::new();
    for tag in tags {
        let (mut data, mut size, mut context, mut exists) = (std::ptr::null_mut::<c_void>(), 0u32, std::ptr::null_mut(), BOOL(0));
        // SAFETY: DirectWrite's tags are little-endian. The table stays readable at `data` for
        // `size` bytes until released with its context.
        unsafe {
            face.TryGetFontTable(u32::from_le_bytes(*tag), &mut data, &mut size, &mut context, &mut exists).ok()?;
            if exists.as_bool() {
                tables.push((*tag, std::slice::from_raw_parts(data.cast::<u8>(), size as usize).to_vec()));
                face.ReleaseFontTable(context);
            }
        }
    }
    Some(Found::Tables(tables))
}
