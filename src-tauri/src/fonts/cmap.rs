//! Which glyph each character is drawn with (`cmap`), from the subtable HarfBuzz, and so the
//! web view, picks: Unicode ones covering all planes first, then those of the BMP, then a
//! symbol font's.

use super::read::Data;

/// The subtables by preference, as (platform, encoding).
const PREFERENCE: [(u16, u16); 8] = [(3, 10), (0, 6), (0, 4), (3, 1), (0, 3), (0, 2), (0, 1), (0, 0)];
/// Symbol fonts map their characters to U+F000–U+F0FF, which HarfBuzz also takes for U+0000–U+00FF.
const SYMBOL: (u16, u16) = (3, 0);

/// The characters `cmap` maps, with their glyphs, sorted by character: none if it has no
/// subtable of a supported encoding and format (0, 4, 6, 12, 13).
pub fn mappings(cmap: &[u8]) -> Vec<(u32, u16)> {
    let cmap = Data(cmap);
    let subtable = |wanted: (u16, u16)| {
        let count = usize::from(cmap.u16(2)?);
        (0..count).find_map(|i| {
            let record = 4 + i * 8;
            ((cmap.u16(record)?, cmap.u16(record + 2)?) == wanted).then(|| cmap.offset32(record + 4)).flatten()
        })
    };
    let mut all = Vec::new();
    if let Some(table) = PREFERENCE.iter().find_map(|&wanted| subtable(wanted)) {
        read_subtable(table, &mut all);
    } else if let Some(table) = subtable(SYMBOL) {
        read_subtable(table, &mut all);
        let low: Vec<(u32, u16)> = all.iter().filter(|(c, _)| (0xF000..=0xF0FF).contains(c)).map(|&(c, g)| (c - 0xF000, g)).collect();
        all.extend(low);
    }
    all.retain(|&(_, glyph)| glyph != 0);
    // The first mapping of a character wins, as a lookup would find it.
    all.sort_by_key(|&(c, _)| c);
    all.dedup_by_key(|&mut (c, _)| c);
    all
}

/// Adds the mappings of a subtable; one of an unsupported format, or cut short, adds those read.
fn read_subtable(table: Data, out: &mut Vec<(u32, u16)>) -> Option<()> {
    match table.u16(0)? {
        0 => {
            for c in 0..256u16 {
                out.push((u32::from(c), u16::from(*table.0.get(6 + usize::from(c))?)));
            }
        }
        4 => {
            let segments = usize::from(table.u16(6)? / 2);
            let (ends, starts, deltas, range_offsets) = (14, 16 + segments * 2, 16 + segments * 4, 16 + segments * 6);
            for segment in 0..segments {
                let (start, end) = (table.u16(starts + segment * 2)?, table.u16(ends + segment * 2)?);
                let delta = table.u16(deltas + segment * 2)?;
                let range_offset = usize::from(table.u16(range_offsets + segment * 2)?);
                for c in start..=end.max(start) {
                    let glyph = if range_offset == 0 {
                        c.wrapping_add(delta)
                    } else {
                        // Counted from where the offset itself is stored.
                        let at = range_offsets + segment * 2 + range_offset + usize::from(c - start) * 2;
                        match table.u16(at)? {
                            0 => 0,
                            glyph => glyph.wrapping_add(delta),
                        }
                    };
                    out.push((u32::from(c), glyph));
                }
            }
        }
        6 => {
            let (first, count) = (table.u16(6)?, table.u16(8)?);
            for i in 0..count {
                out.push((u32::from(first) + u32::from(i), table.u16(10 + usize::from(i) * 2)?));
            }
        }
        format @ (12 | 13) => {
            let groups = usize::try_from(table.u32(12)?).ok()?;
            for group in 0..groups {
                let at = 16 + group * 12;
                let (start, end, glyph) = (table.u32(at)?, table.u32(at + 4)?.min(0x10_FFFF), table.u32(at + 8)?);
                for c in start..=end {
                    let glyph = if format == 12 { glyph + (c - start) } else { glyph };
                    out.push((c, u16::try_from(glyph).ok()?));
                }
            }
        }
        _ => {}
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `cmap` with the subtables given as (platform, encoding, subtable).
    pub fn cmap(subtables: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0, 0];
        out.extend_from_slice(&(subtables.len() as u16).to_be_bytes());
        let mut offset = 4 + subtables.len() * 8;
        for (platform, encoding, table) in subtables {
            out.extend_from_slice(&platform.to_be_bytes());
            out.extend_from_slice(&encoding.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            offset += table.len();
        }
        for (_, _, table) in subtables {
            out.extend_from_slice(table);
        }
        out
    }

    fn words(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// Format 12 with groups of (first character, last, first glyph).
    fn format12(groups: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut out = words(&[12, 0]);
        out.extend_from_slice(&((16 + groups.len() * 12) as u32).to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(groups.len() as u32).to_be_bytes());
        for &(start, end, glyph) in groups {
            for value in [start, end, glyph] {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out
    }

    #[test]
    fn reads_format_4() {
        // Segments: 'A'..'C' by delta, '0'..'1' through the glyph array, and the last 0xFFFF.
        let table = words(&[
            4,
            0,
            0,
            6,
            0,
            0,
            0, // format, length, language, segCountX2, search hints
            0x43,
            0x31,
            0xFFFF, // ends
            0,      // reserved
            0x41,
            0x30,
            0xFFFF, // starts
            (10u16).wrapping_sub(0x41),
            0,
            1, // deltas
            0,
            4,
            0, // range offsets: the second's array starts 4 bytes after its offset
            20,
            0, // glyphs of '0' and '1' ('1' has none)
        ]);
        let map = mappings(&cmap(&[(3, 1, table)]));
        assert_eq!(map, [(0x30, 20), (0x41, 10), (0x42, 11), (0x43, 12)]);
    }

    #[test]
    fn reads_format_12_and_prefers_it() {
        let bmp = words(&[6, 10, 0, 0x41, 2, 7, 8]);
        let full = format12(&[(0x41, 0x42, 30), (0x1F600, 0x1F600, 40)]);
        // The full subtable wins wherever it is listed.
        let map = mappings(&cmap(&[(3, 1, bmp.clone()), (3, 10, full)]));
        assert_eq!(map, [(0x41, 30), (0x42, 31), (0x1F600, 40)]);
        assert_eq!(mappings(&cmap(&[(3, 1, bmp)])), [(0x41, 7), (0x42, 8)]);
    }

    #[test]
    fn maps_symbol_fonts_low_characters_too() {
        let symbol = words(&[6, 10, 0, 0xF041, 1, 9]);
        assert_eq!(mappings(&cmap(&[(3, 0, symbol)])), [(0x41, 9), (0xF041, 9)]);
    }

    #[test]
    fn ignores_what_it_cant_read() {
        // A Mac Roman subtable only, then a subtable cut short.
        assert!(mappings(&cmap(&[(1, 0, vec![0; 262])])).is_empty());
        let cut = format12(&[(0x41, 0x41, 5), (0x42, 0x42, 6)]);
        assert_eq!(mappings(&cmap(&[(3, 10, cut[..cut.len() - 4].to_vec())])), [(0x41, 5)]);
        assert!(mappings(b"").is_empty());
    }
}
