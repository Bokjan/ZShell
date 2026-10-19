//! A font's substitutions in the compact form the frontend applies them in
//! (`lib/fontLigatures.ts`, which describes how it applies them).
//!
//! Only the glyphs the substitutions tell apart keep a number of their own: those they match
//! (in a coverage, a class other than 0, a rule or a ligature) and those whose glyph class
//! their lookups skip glyphs by. They are numbered from 0 in the order of their glyph ids, so
//! that ranges of glyphs stay ranges, and every other glyph is `glyph_count`, which no rule
//! matches. A font's rules are then a few KB to a few hundred, where its `cmap` and `GSUB`
//! are hundreds.
//!
//! Everything else is one array of numbers, `data`, in which tables point to others by
//! index (`NONE` for none):
//!
//! - glyph ranges (a coverage, a class definition): `count, (start, end, value)…`, where
//!   `end` is inclusive and `value` is the coverage index of `start` or the class
//! - a lookup: `flag, mark filtering set (a coverage), reverse (0 or 1), count, subtable…`
//! - a subtable, by its kind (`KINDS`):
//!   - single: `kind, coverage, count, substitute…` (by coverage index; `NONE` for none)
//!   - multiple: `kind, coverage, count, sequence…`, a sequence being `count, glyph…`
//!   - ligature: `kind, coverage, count, set…`; a set `count, ligature…`; a ligature
//!     `glyph, count, component…` (the components after the first)
//!   - glyph rules: `kind, coverage, count, rule set…`
//!   - class rules: `kind, coverage, backtrack classes, input classes, lookahead classes,
//!     count, rule set…`, the sets by the first glyph's input class
//!   - coverage rules: `kind, count, backtrack coverage…, count, input coverage…, count,
//!     lookahead coverage…, count, (sequence index, lookup)…`
//!   - reverse: `kind, coverage, count, backtrack coverage…, count, lookahead coverage…,
//!     count, substitute…`
//! - a rule set: `count, rule…`; a rule: `count, backtrack…, count, input…, count,
//!   lookahead…, count, (sequence index, lookup)…`, with glyphs or classes, the input
//!   without the first glyph and the backtrack closest first
//!
//! Lookups are numbered by their place in `lookups`, which gives where each starts.

use std::collections::HashMap;

use serde::Serialize;
use ts_rs::TS;

use super::gsub::{GlyphClasses, Ranges, Record, Rule, Substitutions, Subtable};

/// No table, glyph or lookup.
pub const NONE: u32 = u32::MAX;

/// The kinds of subtables in `data`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Kinds {
    single: u32,
    multiple: u32,
    ligature: u32,
    glyph_rules: u32,
    class_rules: u32,
    coverage_rules: u32,
    reverse: u32,
}

pub const KINDS: Kinds = Kinds { single: 1, multiple: 2, ligature: 3, glyph_rules: 4, class_rules: 5, coverage_rules: 6, reverse: 7 };

/// The lookup flags that skip glyphs by their class, and HarfBuzz's glyph properties they look
/// at (`GDEF` classes 1 to 3, and a mark's attachment class above).
const IGNORE_BASE: u16 = 0x02;
const IGNORE_LIGATURES: u16 = 0x04;
const IGNORE_MARKS: u16 = 0x08;
const MARK_FILTERING_SET: u16 = 0x10;
const MARK_ATTACHMENT_TYPE: u16 = 0xFF00;

/// The substitutions of a terminal's font (see the module).
#[derive(Serialize, TS, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LigatureRules {
    /// The family found, one of those asked for.
    pub family: String,
    /// The number of glyphs the rules tell apart, which also stands for any other.
    pub glyph_count: u32,
    /// The glyphs of characters, by runs of characters in triples: the first, the last, and the
    /// first's glyph, which the others' follow on from (unless it is `glyph_count`, the glyph of
    /// all). Only where a glyph the rules tell apart, or `glyph_count` for `missing_glyph`, is.
    pub chars: Vec<u32>,
    /// The glyph of characters not in `chars` (that of glyph 0, which characters the font lacks get).
    pub missing_glyph: u32,
    /// HarfBuzz's properties of each glyph (and of `glyph_count`), as far as lookup flags look
    /// at them; empty if none does.
    pub props: Vec<u32>,
    /// Where each lookup starts in `data`.
    pub lookups: Vec<u32>,
    /// The lookups applied, in order.
    pub applied: Vec<u32>,
    pub data: Vec<u32>,
}

/// The rules of a font with these `cmap` mappings, substitutions and glyph classes.
pub fn compile(family: &str, chars: &[(u32, u16)], substitutions: &Substitutions, classes: &GlyphClasses) -> LigatureRules {
    compile_keeping(family, chars, substitutions, classes, false).0
}

/// The rules with every glyph numbered (`keep_all`, for tests), and the glyph id of each number.
pub fn compile_keeping(
    family: &str,
    chars: &[(u32, u16)],
    substitutions: &Substitutions,
    classes: &GlyphClasses,
    keep_all: bool,
) -> (LigatureRules, Vec<u16>) {
    let lookups = &substitutions.lookups;
    let flags = lookups.values().fold(0, |flags, lookup| flags | lookup.flag);
    // The properties HarfBuzz gives each glyph, without those no lookup looks at.
    let mut wanted = flags & (IGNORE_BASE | IGNORE_LIGATURES);
    if flags & (IGNORE_MARKS | MARK_FILTERING_SET | MARK_ATTACHMENT_TYPE) != 0 {
        wanted |= IGNORE_MARKS | (flags & MARK_ATTACHMENT_TYPE);
    }
    let class_of = |ranges: &Ranges, glyph: u16| ranges.0.iter().find(|r| r.start <= glyph && glyph <= r.end).map_or(0, |r| r.value);
    let props = |glyph: u16| -> u16 {
        let props = match class_of(&classes.classes, glyph) {
            1 => IGNORE_BASE,
            2 => IGNORE_LIGATURES,
            3 => IGNORE_MARKS | (class_of(&classes.mark_attachment, glyph) << 8),
            _ => 0,
        };
        props & wanted
    };

    let mut kept = vec![false; 0x1_0000];
    let mut keep = |glyph: u16| kept[usize::from(glyph)] = true;
    for range in &classes.classes.0 {
        (range.start..=range.end).filter(|&glyph| props(glyph) != 0).for_each(&mut keep);
    }
    for lookup in lookups.values() {
        if let Some(set) = lookup
            .mark_filtering_set
            .and_then(|set| classes.mark_sets.get(usize::from(set)))
            .filter(|_| lookup.flag & MARK_FILTERING_SET != 0)
        {
            set.glyphs().for_each(&mut keep);
        }
        for subtable in &lookup.subtables {
            matched_glyphs(subtable, &mut keep);
            if keep_all {
                output_glyphs(subtable, &mut keep);
            }
        }
    }
    if keep_all {
        chars.iter().for_each(|&(_, glyph)| keep(glyph));
        keep(0);
    }
    let ids: Vec<u16> = (0..=u16::MAX).filter(|&glyph| kept[usize::from(glyph)]).collect();
    let mut numbers = vec![NONE; 0x1_0000];
    for (number, &glyph) in ids.iter().enumerate() {
        numbers[usize::from(glyph)] = number as u32;
    }
    let other = ids.len() as u32;
    let number = |glyph: u16| match numbers[usize::from(glyph)] {
        NONE => other,
        number => number,
    };

    let mut out = Encoder { data: Vec::new(), shared: HashMap::new(), number: &number };
    let ids_by_index: HashMap<u16, u32> = lookups.keys().enumerate().map(|(id, &index)| (index, id as u32)).collect();
    let lookup_starts = lookups
        .values()
        .map(|lookup| {
            let set = lookup.mark_filtering_set.filter(|_| lookup.flag & MARK_FILTERING_SET != 0);
            let set = set.and_then(|set| classes.mark_sets.get(usize::from(set))).map_or(NONE, |set| out.ranges(set));
            let subtables: Vec<u32> = lookup.subtables.iter().map(|subtable| out.subtable(subtable, &ids_by_index)).collect();
            let start = out.data.len() as u32;
            out.data.extend([u32::from(lookup.flag), set, u32::from(lookup.reverse), subtables.len() as u32]);
            out.data.extend(subtables);
            start
        })
        .collect();
    let props: Vec<u32> = ids.iter().map(|&glyph| u32::from(props(glyph))).chain([0]).collect();
    let rules = LigatureRules {
        family: family.to_owned(),
        glyph_count: other,
        chars: char_runs(chars, &number, other, number(0) != other),
        missing_glyph: number(0),
        props: if props.iter().any(|&p| p != 0) { props } else { Vec::new() },
        lookups: lookup_starts,
        applied: substitutions.applied.iter().map(|index| ids_by_index[index]).collect(),
        data: out.data,
    };
    (rules, ids)
}

/// The runs of `chars` (see [`LigatureRules::chars`]), with the characters of glyphs that have
/// no number only if `others` (when glyph 0, that of the characters the font lacks, has one).
fn char_runs(chars: &[(u32, u16)], number: &impl Fn(u16) -> u32, other: u32, others: bool) -> Vec<u32> {
    let mut runs: Vec<[u32; 3]> = Vec::new();
    for &(c, glyph) in chars {
        let glyph = number(glyph);
        if glyph == other && !others {
            continue;
        }
        match runs.last_mut() {
            Some([first, last, start])
                if c == *last + 1 && (if *start == other { glyph == other } else { glyph == *start + (c - *first) }) =>
            {
                *last = c
            }
            _ => runs.push([c, c, glyph]),
        }
    }
    runs.concat()
}

/// Calls `keep` with each glyph `subtable` matches: those of its coverages and classes (other
/// than 0), of its rules on glyphs, and the components of its ligatures.
fn matched_glyphs(subtable: &Subtable, keep: &mut impl FnMut(u16)) {
    let mut all = |ranges: &Ranges| ranges.glyphs().for_each(&mut *keep);
    match subtable {
        Subtable::Single { coverage, .. } | Subtable::Multiple { coverage, .. } => all(coverage),
        Subtable::Ligature { coverage, sets } => {
            all(coverage);
            sets.iter().flatten().flat_map(|(_, components)| components).for_each(|&glyph| keep(glyph));
        }
        Subtable::GlyphRules { coverage, sets } => {
            all(coverage);
            sets.iter()
                .flatten()
                .flat_map(|rule| rule.backtrack.iter().chain(&rule.input).chain(&rule.lookahead))
                .for_each(|&glyph| keep(glyph));
        }
        Subtable::ClassRules { coverage, backtrack, input, lookahead, .. } => {
            [coverage, backtrack, input, lookahead].into_iter().for_each(all)
        }
        Subtable::CoverageRules { backtrack, input, lookahead, .. } => backtrack.iter().chain(input).chain(lookahead).for_each(all),
        Subtable::Reverse { coverage, backtrack, lookahead, .. } => [coverage].into_iter().chain(backtrack).chain(lookahead).for_each(all),
    }
}

/// Calls `keep` with each glyph `subtable` substitutes.
fn output_glyphs(subtable: &Subtable, keep: &mut impl FnMut(u16)) {
    match subtable {
        Subtable::Single { substitutes, .. } => substitutes.iter().flatten().for_each(|&glyph| keep(glyph)),
        Subtable::Multiple { sequences, .. } => sequences.iter().flatten().for_each(|&glyph| keep(glyph)),
        Subtable::Ligature { sets, .. } => sets.iter().flatten().for_each(|&(glyph, _)| keep(glyph)),
        Subtable::Reverse { substitutes, .. } => substitutes.iter().for_each(|&glyph| keep(glyph)),
        _ => {}
    }
}

struct Encoder<'a, F> {
    data: Vec<u32>,
    /// Tables already written, which identical ones point to instead.
    shared: HashMap<Vec<u32>, u32>,
    number: &'a F,
}

impl<F: Fn(u16) -> u32> Encoder<'_, F> {
    fn table(&mut self, table: Vec<u32>) -> u32 {
        if let Some(&start) = self.shared.get(&table) {
            return start;
        }
        let start = self.data.len() as u32;
        self.data.extend_from_slice(&table);
        self.shared.insert(table, start);
        start
    }

    /// A coverage or a class definition, whose glyphs all have a number.
    fn ranges(&mut self, ranges: &Ranges) -> u32 {
        let mut table = vec![ranges.0.len() as u32];
        for range in &ranges.0 {
            table.extend([(self.number)(range.start), (self.number)(range.end), u32::from(range.value)]);
        }
        self.table(table)
    }

    fn glyphs(&mut self, glyphs: &[u16]) -> Vec<u32> {
        glyphs.iter().map(|&glyph| (self.number)(glyph)).collect()
    }

    fn records(&self, records: &[Record], ids: &HashMap<u16, u32>) -> Vec<u32> {
        let mut out = vec![records.len() as u32];
        for record in records {
            out.extend([u32::from(record.sequence_index), ids.get(&record.lookup_index).copied().unwrap_or(NONE)]);
        }
        out
    }

    fn rule_sets(&mut self, sets: &[Vec<Rule>], glyphs: bool, ids: &HashMap<u16, u32>) -> Vec<u32> {
        let mut out = vec![sets.len() as u32];
        for set in sets {
            let mut table = vec![set.len() as u32];
            for rule in set {
                let mut encoded = Vec::new();
                for sequence in [&rule.backtrack, &rule.input, &rule.lookahead] {
                    encoded.push(sequence.len() as u32);
                    if glyphs {
                        encoded.extend(self.glyphs(sequence));
                    } else {
                        encoded.extend(sequence.iter().map(|&class| u32::from(class)));
                    }
                }
                encoded.extend(self.records(&rule.records, ids));
                table.push(self.table(encoded));
            }
            out.push(self.table(table));
        }
        out
    }

    fn coverage_list(&mut self, list: &[Ranges]) -> Vec<u32> {
        let mut out = vec![list.len() as u32];
        for coverage in list {
            out.push(self.ranges(coverage));
        }
        out
    }

    fn subtable(&mut self, subtable: &Subtable, ids: &HashMap<u16, u32>) -> u32 {
        let table = match subtable {
            Subtable::Single { coverage, substitutes } => {
                let mut table = vec![KINDS.single, self.ranges(coverage), substitutes.len() as u32];
                table.extend(substitutes.iter().map(|glyph| glyph.map_or(NONE, self.number)));
                table
            }
            Subtable::Multiple { coverage, sequences } => {
                let mut table = vec![KINDS.multiple, self.ranges(coverage), sequences.len() as u32];
                for sequence in sequences {
                    let encoded = [vec![sequence.len() as u32], self.glyphs(sequence)].concat();
                    table.push(self.table(encoded));
                }
                table
            }
            Subtable::Ligature { coverage, sets } => {
                let mut table = vec![KINDS.ligature, self.ranges(coverage), sets.len() as u32];
                for set in sets {
                    let mut encoded = vec![set.len() as u32];
                    for (glyph, components) in set {
                        let ligature = [vec![(self.number)(*glyph), components.len() as u32], self.glyphs(components)].concat();
                        encoded.push(self.table(ligature));
                    }
                    table.push(self.table(encoded));
                }
                table
            }
            Subtable::GlyphRules { coverage, sets } => {
                let start = vec![KINDS.glyph_rules, self.ranges(coverage)];
                [start, self.rule_sets(sets, true, ids)].concat()
            }
            Subtable::ClassRules { coverage, backtrack, input, lookahead, sets } => {
                let start =
                    vec![KINDS.class_rules, self.ranges(coverage), self.ranges(backtrack), self.ranges(input), self.ranges(lookahead)];
                [start, self.rule_sets(sets, false, ids)].concat()
            }
            Subtable::CoverageRules { backtrack, input, lookahead, records } => {
                let mut table = vec![KINDS.coverage_rules];
                for list in [backtrack, input, lookahead] {
                    table.extend(self.coverage_list(list));
                }
                table.extend(self.records(records, ids));
                table
            }
            Subtable::Reverse { coverage, backtrack, lookahead, substitutes } => {
                let mut table = vec![KINDS.reverse, self.ranges(coverage)];
                table.extend(self.coverage_list(backtrack));
                table.extend(self.coverage_list(lookahead));
                table.push(substitutes.len() as u32);
                table.extend(self.glyphs(substitutes));
                table
            }
        };
        self.table(table)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::fonts::gsub::{self, GlyphRange};
    use crate::fonts::testing::{chain, classes, coverage, gsub, ligatures, lookup, single, Table};

    /// The characters of the font made up below, and their glyphs.
    const CHARS: [(u32, u16); 11] = [
        ('-' as u32, 1),
        ('<' as u32, 3),
        ('=' as u32, 4),
        ('>' as u32, 2),
        ('a' as u32, 7),
        ('f' as u32, 5),
        ('i' as u32, 6),
        ('q' as u32, 16),
        ('w' as u32, 10),
        ('x' as u32, 9),
        (0x301, 8),
    ];

    /// A font with a substitution of each kind, for these tests and the frontend's
    /// (`lib/fontLigatures.test.ts`, through the fixture written below):
    ///
    /// - `->` becomes an arrow (glyphs 11 and 12), but not after another `-` (`-->`);
    /// - `fi` ligates (13), past marks (glyph 8, U+0301);
    /// - `a` before `w` becomes 14, from the end of the text;
    /// - `<` before `=` becomes 15 (by classes);
    /// - `q` becomes two glyphs (16, 17);
    /// - an arrow before `=` (`->=`) is a rule that changes nothing.
    fn font() -> (Substitutions, GlyphClasses) {
        let ignore = chain(&[&[1]], &[&[1], &[2]], &[], &[]);
        let arrow = chain(&[], &[&[1], &[2]], &[], &[(0, 3), (1, 4)]);
        let reverse = Table::new().u16(1).offset16(coverage(&[7])).offsets(vec![]).offsets(vec![coverage(&[10])]).array(&[14]);
        let class_rule = Table::new().array(&[]).u16(2).u16(2).array(&[]).u16(1).u16s(&[0, 6]);
        let class_rules = Table::new()
            .u16(2)
            .offset16(coverage(&[3]))
            .u16(0)
            .offset16(classes(&[(3, 3, 1), (4, 4, 2)]))
            .u16(0)
            .offsets(vec![Table::new().offsets(vec![]), Table::new().offsets(vec![class_rule])]);
        let multiple = Table::new().u16(1).offset16(coverage(&[16])).offsets(vec![Table::new().array(&[16, 17])]);
        let lookups = vec![
            lookup(6, 0, vec![ignore, arrow]),
            lookup(4, 0x08, vec![ligatures(5, &[(13, &[6])])]),
            lookup(8, 0, vec![reverse]),
            lookup(1, 0, vec![single(&[(1, 11)])]),
            lookup(1, 0, vec![single(&[(2, 12)])]),
            lookup(6, 0, vec![class_rules]),
            lookup(1, 0, vec![single(&[(3, 15)])]),
            lookup(2, 0, vec![multiple]),
            lookup(6, 0, vec![chain(&[], &[&[12]], &[&[4]], &[])]),
        ];
        let table = gsub(&[(b"ccmp", &[7]), (b"liga", &[1]), (b"calt", &[0, 2, 5, 8])], lookups);
        let classes = GlyphClasses { classes: gsub::Ranges(vec![GlyphRange { start: 8, end: 8, value: 3 }]), ..Default::default() };
        (gsub::substitutions(&table), classes)
    }

    #[test]
    fn numbers_only_the_glyphs_the_rules_tell_apart() {
        let (substitutions, classes) = font();
        let (rules, ids) = compile_keeping("Test", &CHARS, &substitutions, &classes, false);
        // Not 9 (`x`), nor the glyphs only substituted, nor glyph 0.
        assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 16]);
        assert_eq!((rules.glyph_count, rules.missing_glyph), (11, 11));
        // In runs: `-`, then `<` and `=` (glyphs 3 and 4) together, but not `>` (glyph 2).
        assert_eq!(&rules.chars[..9], [0x2d, 0x2d, 0, 0x3c, 0x3d, 2, 0x3e, 0x3e, 1]);
        assert_eq!(&rules.chars[9..18], [0x61, 0x61, 6, 0x66, 0x66, 4, 0x69, 0x69, 5]);
        assert!(!rules.chars.chunks(3).any(|run| run[0] == 'x' as u32));
        assert_eq!(rules.applied, [0, 1, 2, 5, 7, 8]);
    }

    #[test]
    fn keeps_the_characters_of_other_glyphs_when_glyph_0_is_told_apart() {
        let table = gsub(&[(b"calt", &[0])], vec![lookup(6, 0, vec![chain(&[&[0, 1]], &[&[2]], &[], &[])])]);
        let rules =
            compile("Test", &[('a' as u32, 5), ('b' as u32, 6), ('c' as u32, 2)], &gsub::substitutions(&table), &GlyphClasses::default());
        // Glyphs 0, 1 and 2 have numbers; `a` and `b` are another glyph, unlike a missing character.
        assert_eq!((rules.glyph_count, rules.missing_glyph), (3, 0));
        assert_eq!(rules.chars, [0x61, 0x62, 3, 0x63, 0x63, 2]);
    }

    #[test]
    fn gives_glyph_properties_only_when_lookups_look_at_them() {
        let (substitutions, classes) = font();
        let rules = compile("Test", &CHARS, &substitutions, &classes);
        // The mark (glyph 8, number 7), and nothing for the others.
        let marks: Vec<usize> = rules.props.iter().enumerate().filter(|(_, &p)| p != 0).map(|(number, _)| number).collect();
        assert_eq!((marks, rules.props[7]), (vec![7], u32::from(IGNORE_MARKS)));
        let table = gsub(&[(b"calt", &[0])], vec![lookup(1, 0, vec![single(&[(8, 9)])])]);
        assert!(compile("Test", &CHARS, &gsub::substitutions(&table), &classes).props.is_empty());
    }

    #[test]
    fn writes_lookups_and_shares_identical_tables() {
        let table = gsub(&[(b"calt", &[0])], vec![lookup(1, 0x10, vec![single(&[(1, 2)]), single(&[(1, 3)])]), lookup(1, 0, vec![])]);
        let mut substitutions = gsub::substitutions(&table);
        substitutions.lookups.get_mut(&0).unwrap().mark_filtering_set = Some(0);
        let classes = GlyphClasses { mark_sets: vec![gsub::Ranges(vec![GlyphRange { start: 3, end: 3, value: 0 }])], ..Default::default() };
        let rules = compile("Test", &[], &substitutions, &classes);
        let data = &rules.data;
        let lookup = rules.lookups[0] as usize;
        assert_eq!(&data[lookup..lookup + 4], [0x10, data[lookup + 1], 0, 2]);
        let (first, second) = (data[lookup + 4] as usize, data[lookup + 5] as usize);
        // Both single substitutions point to the same coverage; glyph 2 is only substituted.
        assert_eq!(data[first..first + 4], [KINDS.single, data[first + 1], 1, rules.glyph_count]);
        assert_eq!(data[first + 1], data[second + 1]);
        let coverage = data[first + 1] as usize;
        assert_eq!(&data[coverage..coverage + 4], [1, 0, 0, 0]);
        // The mark set, whose glyph has a number: glyphs 1 and 3 are numbers 0 and 1.
        let set = data[lookup + 1] as usize;
        assert_eq!(&data[set..set + 4], [1, 1, 1, 0]);
    }

    /// The fixture the frontend's tests apply: the font above, with the glyph id of each number.
    #[test]
    fn bindings_fixture_is_up_to_date() {
        let (substitutions, classes) = font();
        let (rules, ids) = compile_keeping("Test", &CHARS, &substitutions, &classes, false);
        let generated = format!("{}\n", serde_json::json!({ "rules": rules, "glyphs": ids }));
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/fontLigatures.fixture.json");
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(&path, generated).unwrap();
            return;
        }
        let written = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        assert!(
            written == generated,
            "src/lib/fontLigatures.fixture.json is out of date: run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri"
        );
    }
}
