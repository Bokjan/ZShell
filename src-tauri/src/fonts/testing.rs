//! OpenType tables made up for tests: a table is its fields and the tables its offsets point
//! to, written after it.

/// A table being written.
#[derive(Clone, Default)]
pub struct Table {
    data: Vec<u8>,
    /// Where an offset is (of 2 or 4 bytes), and the table it points to.
    links: Vec<(usize, usize, Table)>,
}

impl Table {
    pub fn new() -> Table {
        Table::default()
    }

    pub fn u16(mut self, value: u16) -> Table {
        self.data.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn u16s(self, values: &[u16]) -> Table {
        values.iter().fold(self, |table, &value| table.u16(value))
    }

    /// A count and the values.
    pub fn array(self, values: &[u16]) -> Table {
        self.u16(values.len() as u16).u16s(values)
    }

    pub fn u32(mut self, value: u32) -> Table {
        self.data.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn tag(mut self, tag: &[u8; 4]) -> Table {
        self.data.extend_from_slice(tag);
        self
    }

    pub fn offset16(mut self, table: Table) -> Table {
        self.links.push((self.data.len(), 2, table));
        self.u16(0)
    }

    pub fn offset32(mut self, table: Table) -> Table {
        self.links.push((self.data.len(), 4, table));
        self.u32(0)
    }

    /// A count and an offset to each table.
    pub fn offsets(self, tables: Vec<Table>) -> Table {
        let count = tables.len() as u16;
        tables.into_iter().fold(self.u16(count), Table::offset16)
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut out = self.data.clone();
        for (at, width, table) in &self.links {
            let offset = out.len();
            if *width == 2 {
                out[*at..at + 2].copy_from_slice(&(offset as u16).to_be_bytes());
            } else {
                out[*at..at + 4].copy_from_slice(&(offset as u32).to_be_bytes());
            }
            out.extend(table.bytes());
        }
        out
    }
}

pub fn coverage(glyphs: &[u16]) -> Table {
    Table::new().u16(1).array(glyphs)
}

/// A coverage of format 2, by (first, last) ranges.
pub fn coverage_ranges(ranges: &[(u16, u16)]) -> Table {
    let mut table = Table::new().u16(2).u16(ranges.len() as u16);
    let mut index = 0;
    for &(start, end) in ranges {
        table = table.u16s(&[start, end, index]);
        index += end - start + 1;
    }
    table
}

/// A class definition of format 2, by (first, last, class).
pub fn classes(ranges: &[(u16, u16, u16)]) -> Table {
    ranges.iter().fold(Table::new().u16(2).u16(ranges.len() as u16), |table, &(start, end, class)| table.u16s(&[start, end, class]))
}

pub fn lookup(kind: u16, flag: u16, subtables: Vec<Table>) -> Table {
    Table::new().u16(kind).u16(flag).offsets(subtables)
}

/// Single substitutions of format 2.
pub fn single(pairs: &[(u16, u16)]) -> Table {
    let from: Vec<u16> = pairs.iter().map(|&(from, _)| from).collect();
    let to: Vec<u16> = pairs.iter().map(|&(_, to)| to).collect();
    Table::new().u16(2).offset16(coverage(&from)).array(&to)
}

/// Ligatures of the first glyph `first`: (ligature, the other components).
pub fn ligatures(first: u16, ligatures: &[(u16, &[u16])]) -> Table {
    let ligatures =
        ligatures.iter().map(|&(glyph, components)| Table::new().u16(glyph).u16(components.len() as u16 + 1).u16s(components)).collect();
    Table::new().u16(1).offset16(coverage(&[first])).offsets(vec![Table::new().offsets(ligatures)])
}

/// A chained contextual subtable of format 3: coverages of glyphs, and (position, lookup).
pub fn chain(backtrack: &[&[u16]], input: &[&[u16]], lookahead: &[&[u16]], records: &[(u16, u16)]) -> Table {
    let mut table = Table::new().u16(3);
    for sequence in [backtrack, input, lookahead] {
        table = sequence.iter().fold(table.u16(sequence.len() as u16), |table, glyphs| table.offset16(coverage(glyphs)));
    }
    records.iter().fold(table.u16(records.len() as u16), |table, &(index, lookup)| table.u16s(&[index, lookup]))
}

/// A `GSUB` whose `latn` script has the features given as (tag, lookups), in order.
pub fn gsub(features: &[(&[u8; 4], &[u16])], lookups: Vec<Table>) -> Vec<u8> {
    let indices: Vec<u16> = (0..features.len() as u16).collect();
    let lang_sys = Table::new().u16(0).u16(0xFFFF).array(&indices);
    let script = Table::new().offset16(lang_sys).u16(0);
    let scripts = Table::new().u16(1).tag(b"latn").offset16(script);
    let feature_list = features
        .iter()
        .fold(Table::new().u16(features.len() as u16), |table, (tag, lookups)| table.tag(tag).offset16(Table::new().u16(0).array(lookups)));
    Table::new().u32(0x0001_0000).offset16(scripts).offset16(feature_list).offset16(Table::new().offsets(lookups)).bytes()
}
