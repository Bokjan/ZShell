//! OpenType (sfnt) files, as far as finding ligatures needs them: reading a font's tables
//! (from a font collection too), and writing a font with only those tables.

use std::io::{self, Read, Seek, SeekFrom};

/// A table's tag (`*b"GSUB"`).
pub type Tag = [u8; 4];

/// The tables the frontend finds ligatures with: `cmap` (characters to glyphs) and `GSUB` (the
/// rules), and those opentype.js reads every font with. The outlines (`glyf`, `CFF `), which
/// make up most of a font (42 of the 43 MB of a Sarasa Gothic face), are left out.
pub const TABLES: [Tag; 6] = [*b"cmap", *b"GSUB", *b"head", *b"hhea", *b"hmtx", *b"maxp"];

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    match (data.get(at), data.get(at + 1)) {
        (Some(&a), Some(&b)) => Some(u16::from_be_bytes([a, b])),
        _ => None,
    }
}

fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

/// The tables among `tags` of font `index` in an OpenType file or collection (`.ttc`), read
/// from `file` without reading the rest. Fails on a file that isn't one, or has no such font.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn read_tables(file: &mut (impl Read + Seek), index: u32, tags: &[Tag]) -> io::Result<Vec<(Tag, Vec<u8>)>> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_owned());
    file.seek(SeekFrom::Start(0))?;
    let mut start = 0;
    if &read_u32(file)?.to_be_bytes() == b"ttcf" {
        let _version = read_u32(file)?;
        let count = read_u32(file)?;
        if index >= count {
            return Err(invalid("no such font in the collection"));
        }
        file.seek(SeekFrom::Current(i64::from(index) * 4))?;
        start = u64::from(read_u32(file)?);
    } else if index != 0 {
        return Err(invalid("not a font collection"));
    }
    // The table directory: version, the number of tables and three search hints, then a
    // record of 16 bytes (tag, checksum, offset, length) for each table.
    file.seek(SeekFrom::Start(start + 4))?;
    let mut count = [0; 2];
    file.read_exact(&mut count)?;
    let mut records = vec![0; usize::from(u16::from_be_bytes(count)) * 16];
    file.seek(SeekFrom::Start(start + 12))?;
    file.read_exact(&mut records)?;
    let mut tables = Vec::new();
    for record in records.as_chunks::<16>().0 {
        let tag: Tag = record[..4].try_into().unwrap();
        if !tags.contains(&tag) {
            continue;
        }
        let offset = u32::from_be_bytes(record[8..12].try_into().unwrap());
        let length = u32::from_be_bytes(record[12..16].try_into().unwrap());
        let mut data = vec![0; length as usize];
        file.seek(SeekFrom::Start(u64::from(offset)))?;
        file.read_exact(&mut data)?;
        tables.push((tag, data));
    }
    Ok(tables)
}

/// A TrueType font with `tables` (those of [`TABLES`] the font has) and stand-ins for the
/// others opentype.js requires: outlines with every glyph empty, and no names. `None` if a
/// table besides `GSUB` (a font without ligatures has none) is missing or too short.
pub fn ligature_font(tables: &[(Tag, Vec<u8>)]) -> Option<Vec<u8>> {
    let table = |tag: &Tag| tables.iter().find(|(t, _)| t == tag).map(|(_, data)| data.as_slice());
    if TABLES.iter().any(|tag| tag != b"GSUB" && table(tag).is_none()) {
        return None;
    }
    let glyphs = usize::from(u16_at(table(b"maxp")?, 4)?);
    // `indexToLocFormat`: offsets of 32 bits, or of 16.
    let long_offsets = u16_at(table(b"head")?, 50)? == 1;
    let mut all: Vec<(Tag, Vec<u8>)> = tables.iter().filter(|(tag, _)| TABLES.contains(tag)).cloned().collect();
    all.push((*b"glyf", Vec::new()));
    all.push((*b"loca", vec![0; (glyphs + 1) * if long_offsets { 4 } else { 2 }]));
    // A naming table without names (format 0, no records, the strings right after).
    all.push((*b"name", vec![0, 0, 0, 0, 0, 6]));
    // Glyph names: version 3, which has none.
    let mut post = vec![0; 32];
    post[1] = 3;
    all.push((*b"post", post));
    all.sort_by_key(|(tag, _)| *tag);
    Some(write(&all))
}

/// A TrueType font file with `tables`, sorted by tag.
fn write(tables: &[(Tag, Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    // The search hints: the largest power of two not above the count, and so on.
    let entry_selector = count.ilog2() as u16;
    let search_range = (1 << entry_selector) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for value in [count, search_range, entry_selector, count * 16 - search_range] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut offset = 12 + tables.len() * 16;
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

/// A table's checksum: the sum of its big-endian 32-bit words, the last padded with zeros.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// The tables of a font with `glyphs` glyphs and 16-bit offsets, `GSUB` among them.
    fn tables(glyphs: u16) -> Vec<(Tag, Vec<u8>)> {
        let mut maxp = vec![0, 0, 0x50, 0];
        maxp.extend_from_slice(&glyphs.to_be_bytes());
        vec![
            (*b"maxp", maxp),
            (*b"head", vec![0; 54]),
            (*b"cmap", vec![1, 2, 3]),
            (*b"hhea", vec![4; 36]),
            (*b"hmtx", vec![5; 8]),
            (*b"GSUB", vec![6; 10]),
        ]
    }

    fn collection(fonts: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"ttcf".to_vec();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(fonts.len() as u32).to_be_bytes());
        let mut offset = 12 + fonts.len() * 4;
        for font in fonts {
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            offset += font.len();
        }
        // Each font's table offsets count from the start of the collection.
        for font in fonts {
            let start = out.len() as u32;
            let mut font = font.clone();
            let count = u16_at(&font, 4).unwrap() as usize;
            for record in 0..count {
                let at = 12 + record * 16 + 8;
                let offset = u32::from_be_bytes(font[at..at + 4].try_into().unwrap()) + start;
                font[at..at + 4].copy_from_slice(&offset.to_be_bytes());
            }
            out.extend_from_slice(&font);
        }
        out
    }

    #[test]
    fn writes_the_tables_with_stand_ins_for_the_others() {
        let font = ligature_font(&tables(3)).unwrap();
        let read = read_tables(&mut Cursor::new(&font), 0, &[*b"GSUB", *b"cmap", *b"loca", *b"glyf", *b"name", *b"post"]).unwrap();
        let tags: Vec<&Tag> = read.iter().map(|(tag, _)| tag).collect();
        // In the order of the file, which is by tag.
        assert_eq!(tags, [b"GSUB", b"cmap", b"glyf", b"loca", b"name", b"post"]);
        assert_eq!(read[1].1, [1, 2, 3]);
        assert_eq!(read[2].1, [] as [u8; 0]);
        // An offset of 16 bits for each glyph and one past the last.
        assert_eq!(read[3].1, [0; 8]);
        assert_eq!(&read[5].1[..4], [0, 3, 0, 0]);
        // Ten tables: 8 is the largest power of two below, then 3 and the rest.
        assert_eq!(&font[4..12], [0, 10, 0, 128, 0, 3, 0, 32]);
        // Tables start at multiples of four.
        let records = &font[12..12 + 10 * 16];
        assert!(records.chunks(16).all(|record| u32::from_be_bytes(record[8..12].try_into().unwrap()) % 4 == 0));
    }

    #[test]
    fn needs_every_table_but_the_rules() {
        let mut without_gsub = tables(3);
        without_gsub.retain(|(tag, _)| tag != b"GSUB");
        assert!(ligature_font(&without_gsub).is_some());
        let mut without_cmap = tables(3);
        without_cmap.retain(|(tag, _)| tag != b"cmap");
        assert!(ligature_font(&without_cmap).is_none());
        let mut short_head = tables(3);
        short_head[1].1.truncate(10);
        assert!(ligature_font(&short_head).is_none());
    }

    #[test]
    fn uses_long_offsets_when_the_font_does() {
        let mut long = tables(3);
        long[1].1[51] = 1;
        let font = ligature_font(&long).unwrap();
        let read = read_tables(&mut Cursor::new(&font), 0, &[*b"loca"]).unwrap();
        assert_eq!(read[0].1, [0; 16]);
    }

    #[test]
    fn reads_a_font_of_a_collection() {
        let mut second = tables(7);
        second[2].1 = vec![9, 9];
        let file = collection(&[ligature_font(&tables(3)).unwrap(), ligature_font(&second).unwrap()]);
        let read = |index| read_tables(&mut Cursor::new(&file), index, &[*b"cmap"]).map(|tables| tables[0].1.clone());
        assert_eq!(read(0).unwrap(), [1, 2, 3]);
        assert_eq!(read(1).unwrap(), [9, 9]);
        assert!(read(2).is_err());
        // A single font has no other.
        let single = ligature_font(&tables(3)).unwrap();
        assert!(read_tables(&mut Cursor::new(&single), 1, &[*b"cmap"]).is_err());
        assert!(read_tables(&mut Cursor::new(b"not a font"), 0, &[*b"cmap"]).is_err());
    }

    #[test]
    fn checksums_pad_the_last_word() {
        assert_eq!(checksum(&[0, 0, 0, 1, 0, 0, 0, 2]), 3);
        assert_eq!(checksum(&[1]), 0x0100_0000);
        assert_eq!(checksum(&[0xff; 8]), 0xffff_fffe);
    }
}
