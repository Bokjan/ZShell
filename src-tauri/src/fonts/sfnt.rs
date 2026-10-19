//! OpenType (sfnt) files, as far as finding ligatures needs them: reading a font's tables, from
//! a font collection too.

use std::io::{self, Read, Seek, SeekFrom};

/// A table's tag (`*b"GSUB"`).
pub type Tag = [u8; 4];

/// The tables ligatures are found with: `cmap` (characters to glyphs), `GSUB` (the rules)
/// and `GDEF` (the glyph classes the rules skip glyphs by). The outlines (`glyf`, `CFF `),
/// which make up most of a font (42 of the 43 MB of a Sarasa Gothic face), are left out.
pub const TABLES: [Tag; 3] = [*b"cmap", *b"GDEF", *b"GSUB"];

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
        // Read as far as the file goes, not into a buffer of the length a broken file gives.
        let mut data = Vec::new();
        file.seek(SeekFrom::Start(u64::from(offset)))?;
        file.by_ref().take(u64::from(length)).read_to_end(&mut data)?;
        if data.len() != length as usize {
            return Err(invalid("a table past the end of the file"));
        }
        tables.push((tag, data));
    }
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// A font file with `tables`, without checksums.
    fn font(tables: &[(Tag, Vec<u8>)]) -> Vec<u8> {
        let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + tables.len() * 16;
        for (tag, data) in tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
        }
        tables.iter().for_each(|(_, data)| out.extend_from_slice(data));
        out
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
            let count = usize::from(u16::from_be_bytes([font[4], font[5]]));
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
    fn reads_the_tables_asked_for() {
        let file = font(&[(*b"GSUB", vec![6; 10]), (*b"cmap", vec![1, 2, 3]), (*b"glyf", vec![9; 100])]);
        let read = read_tables(&mut Cursor::new(&file), 0, &TABLES).unwrap();
        assert_eq!(read, [(*b"GSUB", vec![6; 10]), (*b"cmap", vec![1, 2, 3])]);
    }

    #[test]
    fn reads_a_font_of_a_collection() {
        let file = collection(&[font(&[(*b"cmap", vec![1, 2, 3])]), font(&[(*b"GDEF", vec![0]), (*b"cmap", vec![9, 9])])]);
        let read = |index| read_tables(&mut Cursor::new(&file), index, &[*b"cmap"]).map(|tables| tables[0].1.clone());
        assert_eq!(read(0).unwrap(), [1, 2, 3]);
        assert_eq!(read(1).unwrap(), [9, 9]);
        assert!(read(2).is_err());
        // A single font has no other.
        let single = font(&[(*b"cmap", vec![1])]);
        assert!(read_tables(&mut Cursor::new(&single), 1, &[*b"cmap"]).is_err());
        assert!(read_tables(&mut Cursor::new(b"not a font"), 0, &[*b"cmap"]).is_err());
        // A table longer than the file.
        let mut long = font(&[(*b"cmap", vec![1, 2])]);
        long[12 + 12..12 + 16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(read_tables(&mut Cursor::new(&long), 0, &[*b"cmap"]).is_err());
    }
}
