//! The glyph substitutions (`GSUB`) a web view applies to plain text, and the glyph classes
//! (`GDEF`) their lookups skip glyphs by: read into plain structures for [`super::rules`].
//!
//! The lookups are those HarfBuzz applies to horizontal, left-to-right Latin text with no
//! features asked for, in its order: `rvrn` first, then the others (and the language system's
//! required feature) together by lookup index. A subtable that can't be read is left out, as
//! HarfBuzz does with what fails its checks.

use std::collections::BTreeMap;

use super::read::Data;

/// The features applied first, in a stage of their own.
const FIRST_FEATURES: [[u8; 4]; 1] = [*b"rvrn"];
/// The features applied after them: those that ligatures and contextual forms come from,
/// those always applied before them (`ccmp`, `locl`), and the left-to-right forms.
const FEATURES: [[u8; 4]; 9] = [*b"ltra", *b"ltrm", *b"ccmp", *b"locl", *b"rlig", *b"rclt", *b"calt", *b"clig", *b"liga"];
/// The scripts tried, as HarfBuzz does for Latin text. It takes `DFLT` first for punctuation
/// alone (a joined `->` that WebGL draws apart), but fonts with ligatures list the same
/// features under both.
const SCRIPTS: [[u8; 4]; 3] = [*b"latn", *b"DFLT", *b"dflt"];

/// Glyphs by ranges of ids, sorted, each with a value: a coverage index, or a class.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Ranges(pub Vec<GlyphRange>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphRange {
    pub start: u16,
    /// Inclusive.
    pub end: u16,
    /// The coverage index of `start` (those after it follow on), or the class of all.
    pub value: u16,
}

impl Ranges {
    pub fn glyphs(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().flat_map(|range| range.start..=range.end)
    }

    /// The coverage index of `glyph`.
    #[cfg(test)]
    pub fn index(&self, glyph: u16) -> Option<u16> {
        let range = self.0.iter().find(|range| range.start <= glyph && glyph <= range.end)?;
        Some(range.value + (glyph - range.start))
    }
}

/// A rule of a contextual lookup: what comes before the first glyph (closest first), after it
/// in the input, and after the input; the values are glyphs or classes, as the subtable says.
#[derive(Debug, PartialEq)]
pub struct Rule {
    pub backtrack: Vec<u16>,
    pub input: Vec<u16>,
    pub lookahead: Vec<u16>,
    pub records: Vec<Record>,
}

/// A lookup to apply at a position of the input, by index in the lookup list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    pub sequence_index: u16,
    pub lookup_index: u16,
}

#[derive(Debug, PartialEq)]
pub enum Subtable {
    /// Single substitutions by coverage index, and alternate ones as the first alternate
    /// (`None` where there is none), as with a feature's value of 1.
    Single {
        coverage: Ranges,
        substitutes: Vec<Option<u16>>,
    },
    Multiple {
        coverage: Ranges,
        sequences: Vec<Vec<u16>>,
    },
    /// For each first glyph, its ligatures in order, each the ligature and the other components.
    Ligature {
        coverage: Ranges,
        sets: Vec<Vec<(u16, Vec<u16>)>>,
    },
    /// Contextual and chained contextual rules on glyphs, by first glyph.
    GlyphRules {
        coverage: Ranges,
        sets: Vec<Vec<Rule>>,
    },
    /// The same on classes: the rule sets by the first glyph's input class.
    ClassRules {
        coverage: Ranges,
        backtrack: Ranges,
        input: Ranges,
        lookahead: Ranges,
        sets: Vec<Vec<Rule>>,
    },
    /// One rule on coverages, the first glyph's being `input[0]`.
    CoverageRules {
        backtrack: Vec<Ranges>,
        input: Vec<Ranges>,
        lookahead: Vec<Ranges>,
        records: Vec<Record>,
    },
    Reverse {
        coverage: Ranges,
        backtrack: Vec<Ranges>,
        lookahead: Vec<Ranges>,
        substitutes: Vec<u16>,
    },
}

#[derive(Debug, PartialEq)]
pub struct Lookup {
    pub flag: u16,
    pub mark_filtering_set: Option<u16>,
    /// Applied from the end of the text to its start (reverse chaining).
    pub reverse: bool,
    pub subtables: Vec<Subtable>,
}

/// What a font substitutes in plain text.
#[derive(Debug, Default)]
pub struct Substitutions {
    /// The lookups applied, by lookup list index, in order.
    pub applied: Vec<u16>,
    /// Those and the lookups their rules apply, by lookup list index.
    pub lookups: BTreeMap<u16, Lookup>,
}

/// The substitutions of `gsub` (none for a table that can't be read).
pub fn substitutions(gsub: &[u8]) -> Substitutions {
    read_substitutions(Data(gsub)).unwrap_or_default()
}

fn read_substitutions(gsub: Data) -> Option<Substitutions> {
    if gsub.u16(0)? != 1 {
        return None;
    }
    let (scripts, features, lookups) = (gsub.offset16(4)?, gsub.offset16(6)?, gsub.offset16(8)?);
    let lang_sys = SCRIPTS.iter().find_map(|tag| script(scripts, tag))?;
    let feature_count = usize::from(features.u16(0)?);
    let feature_tag = |index: u16| features.tag(2 + usize::from(index) * 6).filter(|_| usize::from(index) < feature_count);
    let feature_lookups = |index: u16| -> Vec<u16> {
        let Some(feature) = features.offset16(2 + usize::from(index) * 6 + 4) else { return Vec::new() };
        feature.u16_array(2).unwrap_or_default()
    };
    // The first of the language system's features with the tag, as HarfBuzz finds it.
    let indices = lang_sys.u16_array(4)?;
    let find = |tag: &[u8; 4]| indices.iter().copied().find(|&index| feature_tag(index) == Some(*tag));
    let required = lang_sys.u16(2).filter(|&index| index != 0xFFFF && feature_tag(index).is_some());
    let required_later = required.is_some_and(|index| feature_tag(index).is_some_and(|tag| FEATURES.contains(&tag)));
    let stage = |tags: &[[u8; 4]], required: Option<u16>| {
        let mut stage: Vec<u16> = tags.iter().filter_map(find).chain(required).flat_map(feature_lookups).collect();
        stage.sort_unstable();
        stage.dedup();
        stage
    };
    let lookup_count = lookups.u16(0)?;
    let mut applied = stage(&FIRST_FEATURES, required.filter(|_| !required_later));
    applied.extend(stage(&FEATURES, required.filter(|_| required_later)));
    applied.retain(|&index| index < lookup_count);

    // The lookups applied and, transitively, those their rules apply.
    let mut all = BTreeMap::new();
    let mut pending = applied.clone();
    while let Some(index) = pending.pop() {
        if index >= lookup_count || all.contains_key(&index) {
            continue;
        }
        let lookup = lookups.offset16(2 + usize::from(index) * 2).and_then(lookup).unwrap_or(Lookup {
            flag: 0,
            mark_filtering_set: None,
            reverse: false,
            subtables: Vec::new(),
        });
        pending.extend(lookup.subtables.iter().flat_map(records).map(|record| record.lookup_index));
        all.insert(index, lookup);
    }
    applied.retain(|index| all.contains_key(index));
    Some(Substitutions { applied, lookups: all })
}

/// The language system used of the script `tag`: a `dflt` one if listed, as HarfBuzz looks for,
/// or the default. Not the page's language: the DOM renderer shapes with the document's and
/// WebGL's canvas with its own, and in the interface's languages fonts with ligatures differ
/// at most by `locl` forms of single letters (Sarasa's for Japanese and Chinese).
fn script<'a>(scripts: Data<'a>, tag: &[u8; 4]) -> Option<Data<'a>> {
    let count = usize::from(scripts.u16(0)?);
    let script = (0..count).find(|&i| scripts.tag(2 + i * 6) == Some(*tag)).and_then(|i| scripts.offset16(2 + i * 6 + 4))?;
    let systems = usize::from(script.u16(2)?);
    (0..systems)
        .find(|&i| script.tag(4 + i * 6) == Some(*b"dflt"))
        .and_then(|i| script.offset16(4 + i * 6 + 4))
        .or_else(|| script.offset16(0))
}

fn records(subtable: &Subtable) -> Vec<Record> {
    match subtable {
        Subtable::GlyphRules { sets, .. } | Subtable::ClassRules { sets, .. } => {
            sets.iter().flatten().flat_map(|rule| rule.records.iter().copied()).collect()
        }
        Subtable::CoverageRules { records, .. } => records.clone(),
        _ => Vec::new(),
    }
}

fn lookup(data: Data) -> Option<Lookup> {
    let (kind, flag, count) = (data.u16(0)?, data.u16(2)?, usize::from(data.u16(4)?));
    let mark_filtering_set = if flag & 0x10 != 0 { Some(data.u16(6 + count * 2)?) } else { None };
    let mut subtables = Vec::new();
    let mut reverse = kind == 8;
    for i in 0..count {
        let Some(mut table) = data.offset16(6 + i * 2) else { continue };
        let mut kind = kind;
        // Extension: the subtable is elsewhere, of the type given here.
        if kind == 7 {
            let Some(extended) = table.u16(2).filter(|_| table.u16(0) == Some(1)).zip(table.offset32(4)) else { continue };
            (kind, table) = extended;
            reverse |= i == 0 && kind == 8;
        }
        if let Some(subtable) = subtable(kind, table) {
            subtables.push(subtable);
        }
    }
    Some(Lookup { flag, mark_filtering_set, reverse, subtables })
}

fn subtable(kind: u16, table: Data) -> Option<Subtable> {
    let format = table.u16(0)?;
    let coverage = || table.offset16(2).and_then(coverage);
    let list = |at: usize| -> Option<Vec<Data>> { (0..usize::from(table.u16(at)?)).map(|i| table.offset16(at + 2 + i * 2)).collect() };
    match (kind, format) {
        (1, 1) => {
            let (coverage, delta) = (coverage()?, table.u16(4)?);
            let substitutes = coverage.glyphs().map(|glyph| Some(glyph.wrapping_add(delta))).collect();
            Some(Subtable::Single { coverage, substitutes })
        }
        (1, 2) => Some(Subtable::Single { coverage: coverage()?, substitutes: table.u16_array(4)?.into_iter().map(Some).collect() }),
        (2, 1) => Some(Subtable::Multiple {
            coverage: coverage()?,
            sequences: list(4)?.into_iter().map(|s| s.u16_array(0)).collect::<Option<_>>()?,
        }),
        (3, 1) => {
            let sets = list(4)?;
            let substitutes =
                sets.into_iter().map(|set| set.u16_array(0).map(|alternates| alternates.first().copied())).collect::<Option<_>>()?;
            Some(Subtable::Single { coverage: coverage()?, substitutes })
        }
        (4, 1) => {
            let sets = list(4)?.into_iter().map(|set| {
                let count = usize::from(set.u16(0)?);
                (0..count)
                    .map(|i| {
                        let ligature = set.offset16(2 + i * 2)?;
                        let components = usize::from(ligature.u16(2)?).checked_sub(1)?;
                        Some((ligature.u16(0)?, ligature.u16s(4, components)?))
                    })
                    .collect::<Option<Vec<_>>>()
            });
            Some(Subtable::Ligature { coverage: coverage()?, sets: sets.collect::<Option<_>>()? })
        }
        (5 | 6, 1) => Some(Subtable::GlyphRules { coverage: coverage()?, sets: rule_sets(table, 4, kind == 6)? }),
        (5, 2) => {
            let classes = class_def(table.offset16(4)?)?;
            let sets = rule_sets(table, 6, false)?;
            Some(Subtable::ClassRules {
                coverage: coverage()?,
                backtrack: Ranges::default(),
                input: classes,
                lookahead: Ranges::default(),
                sets,
            })
        }
        (6, 2) => {
            let class = |at: usize| table.offset16(at).map_or(Some(Ranges::default()), class_def);
            let (backtrack, input, lookahead) = (class(4)?, class(6)?, class(8)?);
            Some(Subtable::ClassRules { coverage: coverage()?, backtrack, input, lookahead, sets: rule_sets(table, 10, true)? })
        }
        (5, 3) => {
            let (glyphs, count) = (usize::from(table.u16(2)?), usize::from(table.u16(4)?));
            let input = coverages(table, 6, glyphs)?;
            let records = read_records(table, 6 + glyphs * 2, count)?;
            (!input.is_empty()).then_some(Subtable::CoverageRules { backtrack: Vec::new(), input, lookahead: Vec::new(), records })
        }
        (6, 3) => {
            let mut at = 2;
            let mut sequence = || -> Option<Vec<Ranges>> {
                let count = usize::from(table.u16(at)?);
                let sequence = coverages(table, at + 2, count)?;
                at += 2 + count * 2;
                Some(sequence)
            };
            let (backtrack, input, lookahead) = (sequence()?, sequence()?, sequence()?);
            let records = read_records(table, at + 2, usize::from(table.u16(at)?))?;
            (!input.is_empty()).then_some(Subtable::CoverageRules { backtrack, input, lookahead, records })
        }
        (8, 1) => {
            let backtrack_count = usize::from(table.u16(4)?);
            let backtrack = coverages(table, 6, backtrack_count)?;
            let at = 6 + backtrack_count * 2;
            let lookahead_count = usize::from(table.u16(at)?);
            let lookahead = coverages(table, at + 2, lookahead_count)?;
            let substitutes = table.u16_array(at + 2 + lookahead_count * 2)?;
            Some(Subtable::Reverse { coverage: coverage()?, backtrack, lookahead, substitutes })
        }
        _ => None,
    }
}

fn coverages(table: Data, at: usize, count: usize) -> Option<Vec<Ranges>> {
    (0..count).map(|i| table.offset16(at + i * 2).and_then(coverage)).collect()
}

fn read_records(table: Data, at: usize, count: usize) -> Option<Vec<Record>> {
    (0..count).map(|i| Some(Record { sequence_index: table.u16(at + i * 4)?, lookup_index: table.u16(at + i * 4 + 2)? })).collect()
}

/// The rule sets of a contextual subtable, listed at `at`; a missing set has no rules.
fn rule_sets(table: Data, at: usize, chained: bool) -> Option<Vec<Vec<Rule>>> {
    let count = usize::from(table.u16(at)?);
    (0..count)
        .map(|i| {
            let Some(set) = table.offset16(at + 2 + i * 2) else { return Some(Vec::new()) };
            (0..usize::from(set.u16(0)?)).map(|i| set.offset16(2 + i * 2).and_then(|rule| read_rule(rule, chained))).collect()
        })
        .collect()
}

fn read_rule(rule: Data, chained: bool) -> Option<Rule> {
    if !chained {
        let (glyphs, count) = (usize::from(rule.u16(0)?), usize::from(rule.u16(2)?));
        let input = rule.u16s(4, glyphs.checked_sub(1)?)?;
        let records = read_records(rule, 4 + input.len() * 2, count)?;
        return Some(Rule { backtrack: Vec::new(), input, lookahead: Vec::new(), records });
    }
    let backtrack = rule.u16_array(0)?;
    let at = 2 + backtrack.len() * 2;
    let input = rule.u16s(at + 2, usize::from(rule.u16(at)?).checked_sub(1)?)?;
    let at = at + 2 + input.len() * 2;
    let lookahead = rule.u16_array(at)?;
    let at = at + 2 + lookahead.len() * 2;
    let records = read_records(rule, at + 2, usize::from(rule.u16(at)?))?;
    Some(Rule { backtrack, input, lookahead, records })
}

pub fn coverage(table: Data) -> Option<Ranges> {
    let mut ranges = Vec::new();
    match table.u16(0)? {
        1 => {
            for (index, glyph) in table.u16_array(2)?.into_iter().enumerate() {
                let index = u16::try_from(index).ok()?;
                match ranges.last_mut() {
                    Some(GlyphRange { end, value, start }) if glyph == end.wrapping_add(1) && index == *value + (*end - *start) + 1 => {
                        *end = glyph
                    }
                    _ => ranges.push(GlyphRange { start: glyph, end: glyph, value: index }),
                }
            }
        }
        2 => {
            for i in 0..usize::from(table.u16(2)?) {
                let at = 4 + i * 6;
                let (start, end, value) = (table.u16(at)?, table.u16(at + 2)?, table.u16(at + 4)?);
                if start <= end {
                    ranges.push(GlyphRange { start, end, value });
                }
            }
        }
        _ => return None,
    }
    ranges.sort_by_key(|range| range.start);
    Some(Ranges(ranges))
}

/// A class definition, without the glyphs of class 0 (those not listed).
pub fn class_def(table: Data) -> Option<Ranges> {
    let mut ranges = Vec::new();
    match table.u16(0)? {
        1 => {
            let start = table.u16(2)?;
            for (i, class) in table.u16_array(4)?.into_iter().enumerate() {
                let glyph = start.checked_add(u16::try_from(i).ok()?)?;
                match ranges.last_mut() {
                    Some(GlyphRange { end, value, .. }) if *value == class && end.checked_add(1) == Some(glyph) => *end = glyph,
                    _ => ranges.push(GlyphRange { start: glyph, end: glyph, value: class }),
                }
            }
        }
        2 => {
            for i in 0..usize::from(table.u16(2)?) {
                let at = 4 + i * 6;
                let (start, end, value) = (table.u16(at)?, table.u16(at + 2)?, table.u16(at + 4)?);
                if start <= end {
                    ranges.push(GlyphRange { start, end, value });
                }
            }
        }
        _ => return None,
    }
    ranges.retain(|range| range.value != 0);
    ranges.sort_by_key(|range| range.start);
    Some(Ranges(ranges))
}

/// The glyph classes of `GDEF` that lookups skip glyphs by.
#[derive(Debug, Default)]
pub struct GlyphClasses {
    /// 1 base, 2 ligature, 3 mark, 4 component.
    pub classes: Ranges,
    pub mark_attachment: Ranges,
    pub mark_sets: Vec<Ranges>,
}

pub fn glyph_classes(gdef: &[u8]) -> GlyphClasses {
    let gdef = Data(gdef);
    let Some(version) = gdef.u32(0) else { return GlyphClasses::default() };
    let class = |at: usize| gdef.offset16(at).and_then(class_def).unwrap_or_default();
    let mark_sets = (version >= 0x0001_0002)
        .then(|| gdef.offset16(12))
        .flatten()
        .and_then(|sets| (0..usize::from(sets.u16(2)?)).map(|i| sets.offset32(4 + i * 4).and_then(coverage)).collect())
        .unwrap_or_default();
    GlyphClasses { classes: class(4), mark_attachment: class(10), mark_sets }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::testing::{chain, classes, coverage, coverage_ranges, gsub, ligatures, lookup, single, Table};

    fn ranges(ranges: &[(u16, u16, u16)]) -> Ranges {
        Ranges(ranges.iter().map(|&(start, end, value)| GlyphRange { start, end, value }).collect())
    }

    fn a_lookup() -> Table {
        lookup(1, 0, vec![single(&[(1, 2)])])
    }

    #[test]
    fn applies_the_features_of_plain_text_in_lookup_order() {
        let lookups = (0..6).map(|_| a_lookup()).collect();
        let table = gsub(&[(b"liga", &[4, 1]), (b"smcp", &[2]), (b"rvrn", &[5]), (b"calt", &[3, 1]), (b"ccmp", &[0])], lookups);
        let found = substitutions(&table);
        // `rvrn` first, then the others together by index; not small capitals.
        assert_eq!(found.applied, [5, 0, 1, 3, 4]);
        assert_eq!(found.lookups.keys().copied().collect::<Vec<_>>(), [0, 1, 3, 4, 5]);
    }

    #[test]
    fn prefers_latin_then_the_default_script_and_a_dflt_language() {
        let feature = |lookup: u16| Table::new().u16(0).array(&[lookup]);
        let features = Table::new().u16(2).tag(b"calt").offset16(feature(0)).tag(b"liga").offset16(feature(1));
        let lookups = Table::new().offsets(vec![a_lookup(), a_lookup()]);
        let lang_sys = |feature: u16| Table::new().u16(0).u16(0xFFFF).array(&[feature]);
        let script = |feature: u16| Table::new().offset16(lang_sys(feature)).u16(0);
        let table =
            |scripts: Table| Table::new().u32(0x0001_0000).offset16(scripts).offset16(features.clone()).offset16(lookups.clone()).bytes();
        // A script other than Latin, then the default one.
        let scripts = Table::new().u16(2).tag(b"cyrl").offset16(script(0)).tag(b"DFLT").offset16(script(1));
        assert_eq!(substitutions(&table(scripts)).applied, [1]);
        // A language system tagged `dflt` comes before the default one.
        let latin = Table::new().offset16(lang_sys(0)).u16(1).tag(b"dflt").offset16(lang_sys(1));
        assert_eq!(substitutions(&table(Table::new().u16(1).tag(b"latn").offset16(latin))).applied, [1]);
        // None of the scripts looked for: nothing applies.
        assert!(substitutions(&table(Table::new().u16(1).tag(b"arab").offset16(script(0)))).applied.is_empty());
    }

    #[test]
    fn takes_the_required_feature_and_the_first_of_a_tag() {
        let features = [(b"calt", 0), (b"calt", 1), (b"abvs", 2)];
        let features = features
            .into_iter()
            .fold(Table::new().u16(3), |table, (tag, lookup)| table.tag(tag).offset16(Table::new().u16(0).array(&[lookup])));
        let lang_sys = Table::new().u16(0).u16(2).array(&[0, 1]);
        let scripts = Table::new().u16(1).tag(b"latn").offset16(Table::new().offset16(lang_sys).u16(0));
        let lookups = Table::new().offsets((0..3).map(|_| a_lookup()).collect());
        let table = Table::new().u32(0x0001_0000).offset16(scripts).offset16(features).offset16(lookups).bytes();
        // The required feature in the first stage, as its tag isn't one applied later.
        assert_eq!(substitutions(&table).applied, [2, 0]);
    }

    #[test]
    fn reads_the_lookups_rules_apply_too() {
        let rule = chain(&[&[1]], &[&[2], &[3]], &[&[4]], &[(1, 1)]);
        let table = gsub(&[(b"calt", &[0])], vec![lookup(6, 0, vec![rule]), lookup(1, 0, vec![single(&[(3, 9)])]), a_lookup()]);
        let found = substitutions(&table);
        assert_eq!(found.applied, [0]);
        assert_eq!(found.lookups.keys().copied().collect::<Vec<_>>(), [0, 1]);
        let expected = Subtable::CoverageRules {
            backtrack: vec![ranges(&[(1, 1, 0)])],
            input: vec![ranges(&[(2, 2, 0)]), ranges(&[(3, 3, 0)])],
            lookahead: vec![ranges(&[(4, 4, 0)])],
            records: vec![Record { sequence_index: 1, lookup_index: 1 }],
        };
        assert_eq!(found.lookups[&0].subtables, [expected]);
        assert_eq!(found.lookups[&1].subtables, [Subtable::Single { coverage: ranges(&[(3, 3, 0)]), substitutes: vec![Some(9)] }]);
    }

    #[test]
    fn reads_substitutions_of_each_kind() {
        let delta = Table::new().u16(1).offset16(coverage(&[5, 6])).u16(10);
        let alternates =
            Table::new().u16(1).offset16(coverage(&[1, 2])).offsets(vec![Table::new().array(&[7, 8]), Table::new().array(&[])]);
        let multiple = Table::new().u16(1).offset16(coverage(&[3])).offsets(vec![Table::new().array(&[4, 5])]);
        let reverse = Table::new().u16(1).offset16(coverage(&[2])).offsets(vec![coverage(&[1])]).offsets(vec![]).array(&[9]);
        let extended = |kind: u16, table: Table| Table::new().u16(1).u16(kind).offset32(table);
        let lookups = vec![
            lookup(1, 0, vec![delta]),
            lookup(3, 0, vec![alternates]),
            lookup(2, 0, vec![multiple]),
            lookup(4, 0, vec![ligatures(1, &[(20, &[2, 3]), (21, &[2])])]),
            lookup(7, 0, vec![extended(8, reverse)]),
            lookup(7, 0, vec![extended(1, single(&[(1, 2)]))]),
        ];
        let found = substitutions(&gsub(&[(b"liga", &[0, 1, 2, 3, 4, 5])], lookups));
        let subtables: Vec<&Subtable> = found.lookups.values().map(|lookup| &lookup.subtables[0]).collect();
        assert_eq!(*subtables[0], Subtable::Single { coverage: ranges(&[(5, 6, 0)]), substitutes: vec![Some(15), Some(16)] });
        // The first alternate, as with a feature's value of 1.
        assert_eq!(*subtables[1], Subtable::Single { coverage: ranges(&[(1, 2, 0)]), substitutes: vec![Some(7), None] });
        assert_eq!(*subtables[2], Subtable::Multiple { coverage: ranges(&[(3, 3, 0)]), sequences: vec![vec![4, 5]] });
        assert_eq!(*subtables[3], Subtable::Ligature { coverage: ranges(&[(1, 1, 0)]), sets: vec![vec![(20, vec![2, 3]), (21, vec![2])]] });
        let reverse = Subtable::Reverse {
            coverage: ranges(&[(2, 2, 0)]),
            backtrack: vec![ranges(&[(1, 1, 0)])],
            lookahead: vec![],
            substitutes: vec![9],
        };
        assert_eq!(*subtables[4], reverse);
        // An extension takes the type of what it extends.
        assert!(found.lookups[&4].reverse && !found.lookups[&5].reverse);
        assert!(matches!(subtables[5], Subtable::Single { .. }));
    }

    #[test]
    fn reads_contextual_rules_on_glyphs_and_classes() {
        // The input count takes in the first glyph, which the coverage gives.
        let rule = Table::new().array(&[1]).u16(2).u16(3).array(&[4]).u16(1).u16s(&[0, 1]);
        let glyph_rules = Table::new().u16(1).offset16(coverage(&[2])).offsets(vec![Table::new().offsets(vec![rule])]);
        let class_rule = Table::new().array(&[]).u16(2).u16(2).array(&[]).u16(0);
        let class_rules = Table::new()
            .u16(2)
            .offset16(coverage(&[2]))
            .u16(0)
            .offset16(classes(&[(2, 3, 1), (5, 5, 0), (6, 6, 2)]))
            .u16(0)
            .offsets(vec![Table::new().offsets(vec![]), Table::new().offsets(vec![class_rule])]);
        // Contextual (not chained) on glyphs: the input count, the lookup count, the rest of the input.
        let context_rule = Table::new().u16(2).u16(0).u16(8);
        let context = Table::new().u16(1).offset16(coverage(&[7])).offsets(vec![Table::new().offsets(vec![context_rule])]);
        let lookups = vec![lookup(6, 0, vec![glyph_rules]), lookup(6, 0, vec![class_rules]), lookup(5, 0, vec![context])];
        let found = substitutions(&gsub(&[(b"calt", &[0, 1, 2])], lookups));
        let rule =
            Rule { backtrack: vec![1], input: vec![3], lookahead: vec![4], records: vec![Record { sequence_index: 0, lookup_index: 1 }] };
        assert_eq!(found.lookups[&0].subtables, [Subtable::GlyphRules { coverage: ranges(&[(2, 2, 0)]), sets: vec![vec![rule]] }]);
        let Subtable::ClassRules { input, backtrack, sets, .. } = &found.lookups[&1].subtables[0] else { panic!() };
        // Class 0 isn't listed; a missing class definition has none.
        assert_eq!(*input, ranges(&[(2, 3, 1), (6, 6, 2)]));
        assert_eq!(*backtrack, Ranges::default());
        assert_eq!(sets[1], [Rule { backtrack: vec![], input: vec![2], lookahead: vec![], records: vec![] }]);
        let Subtable::GlyphRules { sets, .. } = &found.lookups[&2].subtables[0] else { panic!() };
        assert_eq!(sets[0], [Rule { backtrack: vec![], input: vec![8], lookahead: vec![], records: vec![] }]);
    }

    #[test]
    fn leaves_out_what_it_cant_read() {
        // An unknown format, a subtable cut short, and then a good one.
        let cut = Table::new().u16(2).u16(5);
        let table = gsub(&[(b"calt", &[0])], vec![lookup(1, 0, vec![Table::new().u16(9), cut, single(&[(1, 2)])])]);
        assert_eq!(substitutions(&table).lookups[&0].subtables.len(), 1);
        assert!(substitutions(b"").applied.is_empty());
        assert!(substitutions(&table[..20]).applied.is_empty());
    }

    #[test]
    fn reads_coverages_and_classes_as_ranges() {
        let read = |table: Table| coverage_of(&table.bytes());
        fn coverage_of(bytes: &[u8]) -> Ranges {
            super::coverage(Data(bytes)).unwrap()
        }
        assert_eq!(read(coverage(&[3, 4, 5, 9])), ranges(&[(3, 5, 0), (9, 9, 3)]));
        let two = read(coverage_ranges(&[(10, 12), (1, 1)]));
        assert_eq!(two, ranges(&[(1, 1, 3), (10, 12, 0)]));
        assert_eq!((two.index(11), two.index(1), two.index(2)), (Some(1), Some(3), None));
        let format1 = Table::new().u16(1).u16(4).array(&[1, 1, 0, 2]).bytes();
        assert_eq!(class_def(Data(&format1)).unwrap(), ranges(&[(4, 5, 1), (7, 7, 2)]));
    }

    #[test]
    fn reads_glyph_classes() {
        let sets = Table::new().u16(1).u16(1).offset32(coverage(&[8]));
        let gdef = Table::new()
            .u32(0x0001_0002)
            .offset16(classes(&[(1, 5, 1), (8, 9, 3)]))
            .u16s(&[0, 0])
            .offset16(classes(&[(9, 9, 2)]))
            .offset16(sets)
            .bytes();
        let found = glyph_classes(&gdef);
        assert_eq!(found.classes, ranges(&[(1, 5, 1), (8, 9, 3)]));
        assert_eq!(found.mark_attachment, ranges(&[(9, 9, 2)]));
        assert_eq!(found.mark_sets, [ranges(&[(8, 8, 0)])]);
        assert!(glyph_classes(b"").classes.0.is_empty());
    }
}
