//! macOS: a family's tables from CoreText.

use std::ffi::c_void;

use objc2_core_foundation::{kCFTypeSetCallBacks, CFDictionary, CFRetained, CFSet, CFString};
use objc2_core_text::{kCTFontFamilyNameAttribute, kCTFontNameAttribute, CTFont, CTFontDescriptor, CTFontTableOptions};

use super::sfnt::Tag;
use super::Found;

/// The tables among `tags` of `family`'s regular face, if the family is installed; `None` if
/// CoreText fails.
pub fn tables(family: &str, tags: &[Tag]) -> Option<Found> {
    let name = CFString::from_str(family);
    // As WebKit finds a font: by its family name (in any language), then by a face's PostScript
    // name (`Menlo-Regular`).
    // SAFETY: CoreText's constants.
    let attributes = unsafe { [kCTFontFamilyNameAttribute, kCTFontNameAttribute] };
    for attribute in attributes {
        if let Some(descriptor) = installed(attribute, &name)? {
            return Some(Found::Tables(read(&descriptor, tags)));
        }
    }
    Some(Found::Missing)
}

/// A descriptor of the font whose `attribute` is `value`, `Some(None)` if no installed font's
/// is (CoreText would make another font of it), or `None` if CoreText fails.
fn installed(attribute: &CFString, value: &CFString) -> Option<Option<CFRetained<CTFontDescriptor>>> {
    // SAFETY: the dictionary holds strings and the set an attribute's name, as CoreText expects,
    // with the callbacks of a set of CF types.
    unsafe {
        let attributes = CFDictionary::from_slices(&[attribute], &[value]);
        let descriptor = CTFontDescriptor::with_attributes(attributes.as_opaque());
        let mut keys = [(attribute as *const CFString).cast::<c_void>()];
        let mandatory = CFSet::new(None, keys.as_mut_ptr(), 1, &raw const kCFTypeSetCallBacks)?;
        Some(descriptor.matching_font_descriptor(Some(&mandatory)).map(|_| descriptor))
    }
}

/// The tables among `tags` of the font `descriptor` describes.
fn read(descriptor: &CTFontDescriptor, tags: &[Tag]) -> Vec<(Tag, Vec<u8>)> {
    // SAFETY: a descriptor of an installed font, which CoreText makes the font of.
    let font = unsafe { CTFont::with_font_descriptor(descriptor, 0.0, std::ptr::null()) };
    tags.iter()
        // SAFETY: CoreText's tags are the big-endian four bytes.
        .filter_map(|tag| unsafe { font.table(u32::from_be_bytes(*tag), CTFontTableOptions::NoOptions) }.map(|data| (*tag, data.to_vec())))
        .collect()
}
