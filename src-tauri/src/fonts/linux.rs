//! Linux: a family's file from fontconfig, and its tables from the file.

use std::fs::File;
use std::io::BufReader;

use fontconfig::{list_fonts, Fontconfig, Pattern, FC_FAMILY};

use super::sfnt::{self, Tag};
use super::Found;

/// The tables among `tags` of `family`'s regular face (or the nearest one), if the family is
/// installed under that name; `None` if fontconfig fails or the file can't be read.
pub fn tables(family: &str, tags: &[Tag]) -> Option<Found> {
    let fontconfig = Fontconfig::new()?;
    let name = std::ffi::CString::new(family).ok()?;
    let mut pattern = Pattern::new(&fontconfig).ok()?;
    pattern.add_string(FC_FAMILY, &name).ok()?;
    // Matching always finds a font, another family's if need be: listing finds only the fonts
    // with the name (any of theirs, ignoring case).
    if list_fonts(&pattern, None).ok()?.iter().next().is_none() {
        return Some(Found::Missing);
    }
    let font = pattern.font_match().ok()?;
    let path = font.filename().ok()?;
    // The upper 16 bits are a variable font's named instance, which has the same tables.
    let index = font.face_index().unwrap_or(0) as u32 & 0xffff;
    let mut file = BufReader::new(File::open(path).ok()?);
    sfnt::read_tables(&mut file, index, tags).ok().map(Found::Tables)
}
