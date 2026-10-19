//! macOS: a family's tables from CoreText.

use objc2_core_foundation::{CFDictionary, CFString};
use objc2_core_text::{kCTFontFamilyNameAttribute, CTFont, CTFontDescriptor, CTFontTableOptions};

use super::sfnt::Tag;

/// The tables among `tags` of `family`'s regular face, if the family is installed.
pub fn tables(family: &str, tags: &[Tag]) -> Option<Vec<(Tag, Vec<u8>)>> {
    let name = CFString::from_str(family);
    // SAFETY: the attribute is CoreText's constant; the dictionary holds strings, as it expects.
    let font = unsafe {
        let attributes = CFDictionary::from_slices(&[kCTFontFamilyNameAttribute], &[&*name]);
        let descriptor = CTFontDescriptor::with_attributes(attributes.as_opaque());
        CTFont::with_font_descriptor(&descriptor, 0.0, std::ptr::null())
    };
    // CoreText falls back to another font when the family isn't installed.
    // SAFETY: a font CoreText made.
    let found = unsafe { font.family_name() };
    if found.to_string().to_lowercase() != family.to_lowercase() {
        return None;
    }
    let tables = tags
        .iter()
        // SAFETY: CoreText's tags are the big-endian four bytes.
        .filter_map(|tag| unsafe { font.table(u32::from_be_bytes(*tag), CTFontTableOptions::NoOptions) }.map(|data| (*tag, data.to_vec())))
        .collect();
    Some(tables)
}
