import { LIGATURE_NONE as NONE, LIGATURE_SUBTABLES as KIND, type LigatureRules } from "../ipc/bindings";

/**
 * Where ligatures form in a terminal's text, by its font's own substitutions: the backend reads
 * them from the installed font (`fonts/` there, whose `rules.rs` describes the compact form
 * they come in), and this applies them as the web view's shaper (HarfBuzz) does to plain
 * left-to-right text: each lookup in turn over the whole text, skipping the glyphs its flags
 * skip and the default ignorable characters, the subtables of a lookup tried in order.
 *
 * The cells to draw together are then those a substitution changed with the context it was
 * found in: a renderer that draws them as one piece of text has the font shape them again,
 * which gives the same glyphs only if their context is in it too. Each change that spans more
 * than one character gives a range, from the first glyph of its rule's backtrack to the last
 * of its lookahead; any rule found over part of a range, changing glyphs or not (one that
 * keeps a ligature from forming), widens it to its own context, until none does.
 *
 * Two steps of HarfBuzz's are left out, as they only concern accented letters, not the
 * operators programming ligatures join: the normalization before the substitutions (`e` and
 * U+0301 become `é` where the font has it), and the glyph classes it guesses for a font whose
 * `GDEF` has none (fonts with ligatures have them, and most of their lookups skip no glyphs).
 */

export interface FontLigatures {
  /** The parts of `text` to draw together, as [start, end) offsets in UTF-16 code units, sorted. */
  ranges(text: string): [number, number][];
  /** The glyphs of `text` after the substitutions, with where each one's text starts: for tests. */
  shape(text: string): { glyphs: number[]; clusters: number[] };
}

/** Characters HarfBuzz skips while matching (default ignorables), and the joiners among them. */
const IGNORABLE = 1;
const ZWJ = 2;
const ZWNJ = 4;

/**
 * Whether a character is a default ignorable HarfBuzz skips in matching: not the few it keeps in
 * the way (the combining grapheme joiner, Mongolian variation selectors, tags).
 */
export function characterFlags(c: number): number {
  if (c < 0xad) return 0;
  if (c === 0x200d) return IGNORABLE | ZWJ;
  if (c === 0x200c) return IGNORABLE | ZWNJ;
  const ignorable =
    c === 0xad ||
    c === 0x61c ||
    c === 0x17b4 ||
    c === 0x17b5 ||
    c === 0x180e ||
    (c >= 0x200b && c <= 0x200f) ||
    (c >= 0x202a && c <= 0x202e) ||
    (c >= 0x2060 && c <= 0x206f) ||
    (c >= 0xfe00 && c <= 0xfe0f) ||
    c === 0xfeff ||
    (c >= 0xfff0 && c <= 0xfff8) ||
    (c >= 0x1d173 && c <= 0x1d17a) ||
    (c >= 0xe0000 && c <= 0xe0fff && !(c >= 0xe0020 && c <= 0xe007f));
  return ignorable ? IGNORABLE : 0;
}

/** The lookup flags that skip glyphs, and the glyph properties they look at (as `rules.rs` gives them). */
const IGNORE_FLAGS = 0x0e;
const MARK = 0x08;
const USE_MARK_FILTERING_SET = 0x10;
const MARK_ATTACHMENT_TYPE = 0xff00;

/** How deep lookups may apply others, and how many lookups may apply for each glyph, as in HarfBuzz. */
const MAX_NESTING = 64;
const MAX_OPS_FACTOR = 64;
const MIN_OPS = 16384;

/** How a position of a rule is matched: to a glyph, a class, or a coverage. */
const BY_GLYPH = 0;
const BY_CLASS = 1;
const BY_COVERAGE = 2;

export function fontLigatures(rules: LigatureRules): FontLigatures {
  const data = Uint32Array.from(rules.data);
  const props = rules.props.length > 0 ? Uint32Array.from(rules.props) : null;
  const lookups = rules.lookups;
  const applied = rules.applied;
  const missing = rules.missingGlyph;

  // Characters to glyphs: runs of characters, and ASCII in a table.
  const chars = Uint32Array.from(rules.chars);
  const other = rules.glyphCount;
  function glyphOf(c: number): number {
    let low = 0;
    let high = chars.length / 3 - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const at = middle * 3;
      if (c < chars[at]!) high = middle - 1;
      else if (c > chars[at + 1]!) low = middle + 1;
      else return chars[at + 2] === other ? other : chars[at + 2]! + c - chars[at]!;
    }
    return missing;
  }
  const ascii = Uint32Array.from({ length: 128 }, (_, c) => glyphOf(c));

  // For each applied lookup, the glyphs it may start at: as bits, and for each glyph the lookups.
  const words = (rules.glyphCount >> 5) + 1;
  const startBits = new Uint32Array(applied.length * words);
  const startLists: number[][] = Array.from({ length: rules.glyphCount + 1 }, () => []);
  applied.forEach((lookup, k) => {
    const start = lookups[lookup]!;
    for (let s = 0; s < data[start + 3]!; s++) {
      const subtable = data[start + 4 + s]!;
      const kind = data[subtable]!;
      // A rule on coverages starts with its input's first, the others with theirs.
      const inputs = subtable + 2 + data[subtable + 1]!;
      if (kind === KIND.coverageRules && data[inputs] === 0) continue;
      const coverage = kind === KIND.coverageRules ? data[inputs + 1]! : data[subtable + 1]!;
      for (let r = 0; r < data[coverage]!; r++) {
        for (let glyph = data[coverage + 1 + r * 3]!; glyph <= data[coverage + 2 + r * 3]!; glyph++) {
          const bit = k * words + (glyph >> 5);
          if (!(startBits[bit]! & (1 << (glyph & 31)))) startLists[glyph]!.push(k);
          startBits[bit]! |= 1 << (glyph & 31);
        }
      }
    }
  });
  // For each applied lookup, the first and last positions it may start at (none while the first
  // is after the last), kept as substitutions bring the glyphs of later lookups.
  const firstAt = new Int32Array(applied.length);
  const lastAt = new Int32Array(applied.length);
  /** The lookup applying, by its place in `applied`; whether it moved glyphs by adding or removing some. */
  let current = -1;
  let moved = false;

  /** Notes the lookups after the current one the glyph at `at` may start. */
  function mark(at: number) {
    for (const k of startLists[glyphs[at]!]!) {
      if (k <= current) continue;
      if (at < firstAt[k]!) firstAt[k] = at;
      if (at > lastAt[k]!) lastAt[k] = at;
    }
  }

  // The text being shaped: a glyph for each character at first, with where its text starts
  // and ends (merged when glyphs ligate) and its character's flags.
  let size = 0;
  let glyphs = new Uint32Array(64);
  let starts = new Int32Array(64);
  let ends = new Int32Array(64);
  let flags = new Uint8Array(64);
  function reserve(n: number) {
    if (n <= glyphs.length) return;
    const capacity = Math.max(n, glyphs.length * 2);
    const grow = <T extends Uint32Array | Int32Array | Uint8Array>(old: T, next: T) => (next.set(old), next);
    glyphs = grow(glyphs, new Uint32Array(capacity));
    starts = grow(starts, new Int32Array(capacity));
    ends = grow(ends, new Int32Array(capacity));
    flags = grow(flags, new Uint8Array(capacity));
  }
  function remove(at: number) {
    for (const array of [glyphs, starts, ends, flags]) array.copyWithin(at, at + 1, size);
    size--;
  }
  function insertAfter(at: number, count: number) {
    reserve(size + count);
    for (const array of [glyphs, starts, ends, flags]) array.copyWithin(at + 1 + count, at + 1, size);
    size += count;
  }

  // The spans substitutions were found over, and whether they changed any glyph.
  let spanCount = 0;
  let spanStarts = new Int32Array(16);
  let spanEnds = new Int32Array(16);
  let spanChanged = new Uint8Array(16);
  function addSpan(start: number, end: number, changed: boolean) {
    if (spanCount === spanStarts.length) {
      const grow = <T extends Int32Array | Uint8Array>(old: T, next: T) => (next.set(old), next);
      spanStarts = grow(spanStarts, new Int32Array(spanCount * 2));
      spanEnds = grow(spanEnds, new Int32Array(spanCount * 2));
      spanChanged = grow(spanChanged, new Uint8Array(spanCount * 2));
    }
    spanStarts[spanCount] = start;
    spanEnds[spanCount] = end;
    spanChanged[spanCount++] = changed ? 1 : 0;
  }

  /** Counts glyphs changed, so that a rule knows whether the lookups it applied changed any. */
  let changes = 0;
  let ops = 0;
  // The lookup applying, which glyphs it skips, and what a position matches against.
  let lookupId = 0;
  let lookupFlag = 0;
  let markSet = NONE;
  let classes = 0;

  function coverageIndex(table: number, glyph: number): number {
    let low = 0;
    let high = data[table]! - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const at = table + 1 + middle * 3;
      if (glyph < data[at]!) high = middle - 1;
      else if (glyph > data[at + 1]!) low = middle + 1;
      else return data[at + 2]! + glyph - data[at]!;
    }
    return -1;
  }

  function classOf(table: number, glyph: number): number {
    let low = 0;
    let high = data[table]! - 1;
    while (low <= high) {
      const middle = (low + high) >> 1;
      const at = table + 1 + middle * 3;
      if (glyph < data[at]!) high = middle - 1;
      else if (glyph > data[at + 1]!) low = middle + 1;
      else return data[at + 2]!;
    }
    return 0;
  }

  /** Whether the lookup applying looks at a glyph, by its class (HarfBuzz's `check_glyph_property`). */
  function looksAt(glyph: number): boolean {
    if (!props) return true;
    const p = props[glyph]!;
    if (p & lookupFlag & IGNORE_FLAGS) return false;
    if (p & MARK) {
      if (lookupFlag & USE_MARK_FILTERING_SET) return markSet !== NONE && coverageIndex(markSet, glyph) >= 0;
      if (lookupFlag & MARK_ATTACHMENT_TYPE) return (lookupFlag & MARK_ATTACHMENT_TYPE) === (p & MARK_ATTACHMENT_TYPE);
    }
    return true;
  }

  function matches(by: number, value: number, glyph: number): boolean {
    if (by === BY_GLYPH) return glyph === value;
    if (by === BY_CLASS) return classOf(classes, glyph) === value;
    return coverageIndex(value, glyph) >= 0;
  }

  /**
   * The next position after `from` in direction `step` that matches, past those the lookup
   * skips and the ignorable characters that don't match; -1 if there is none. Zero-width
   * non-joiners are skipped only in context (backtrack and lookahead), as in HarfBuzz.
   */
  function next(from: number, step: number, context: boolean, by: number, value: number): number {
    for (let i = from + step; i >= 0 && i < size; i += step) {
      if (!looksAt(glyphs[i]!)) continue;
      if (matches(by, value, glyphs[i]!)) return i;
      const f = flags[i]!;
      if (!(f & IGNORABLE) || (!context && f & ZWNJ)) return -1;
    }
    return -1;
  }

  /** The positions of a rule's input (and of the ligature's components) once matched. */
  let input = new Int32Array(16);

  /**
   * Matches the rule at `rule` (`count, backtrack…, count, input…, count, lookahead…, count,
   * records…`) at `at`, whose glyph has matched the input's first position already; `first` is
   * 1 if the input listed there starts with that position (rules on coverages), 0 if it leaves
   * it out. Returns the position after the input, or -1.
   */
  function applyRule(at: number, rule: number, by: number, classDefs: number, first: number, depth: number): number {
    const backtrack = rule + 1;
    const inputCount = data[backtrack + data[rule]!]!;
    const inputs = backtrack + data[rule]! + 1;
    const lookahead = inputs + inputCount;
    const records = lookahead + 1 + data[lookahead]!;
    const count = inputCount + 1 - first;
    if (input.length < count) input = new Int32Array(count * 2);
    input[0] = at;
    if (by === BY_CLASS) classes = data[classDefs + 1]!;
    for (let k = 1; k < count; k++) {
      const found = next(input[k - 1]!, 1, false, by, data[inputs + first + k - 1]!);
      if (found < 0) return -1;
      input[k] = found;
    }
    let from = at;
    if (by === BY_CLASS) classes = data[classDefs]!;
    for (let k = 0; k < data[rule]!; k++) {
      from = next(from, -1, true, by, data[backtrack + k]!);
      if (from < 0) return -1;
    }
    let to = input[count - 1]!;
    if (by === BY_CLASS) classes = data[classDefs + 2]!;
    for (let k = 0; k < data[lookahead]!; k++) {
      to = next(to, 1, true, by, data[lookahead + 1 + k]!);
      if (to < 0) return -1;
    }
    let start = starts[from]!;
    let end = ends[from]!;
    for (let i = from + 1; i <= to; i++) {
      start = Math.min(start, starts[i]!);
      end = Math.max(end, ends[i]!);
    }
    const before = changes;
    const after = applyRecords(input.slice(0, count), data[records]!, records + 1, input[count - 1]! + 1, depth);
    addSpan(start, end, changes > before);
    return after;
  }

  /**
   * Applies a rule's lookups at the positions of its input (HarfBuzz's `apply_lookup`, which
   * also says how positions move as lookups add or remove glyphs); returns the position after
   * the input.
   */
  function applyRecords(positions: Int32Array, recordCount: number, records: number, end: number, depth: number): number {
    let count = positions.length;
    for (let r = 0; r < recordCount; r++) {
      const index = data[records + r * 2]!;
      const lookup = data[records + r * 2 + 1]!;
      if (index >= count || lookup === NONE || (index === 0 && lookup === lookupId)) continue;
      const length = size;
      if (positions[index]! >= length) continue;
      if (ops++ > maxOps || depth >= MAX_NESTING) break;
      const saved = [lookupId, lookupFlag, markSet, classes] as const;
      const done = applyLookupAt(lookup, positions[index]!, depth + 1) >= 0;
      [lookupId, lookupFlag, markSet, classes] = saved;
      if (!done) continue;
      let delta = size - length;
      if (delta === 0) continue;
      end += delta;
      if (end < positions[index]!) {
        delta += positions[index]! - end;
        end = positions[index]!;
      }
      let after = index + 1;
      if (delta > 0) {
        const grown = new Int32Array(count + delta);
        grown.set(positions);
        positions = grown;
      } else {
        delta = Math.max(delta, after - count);
        after -= delta;
      }
      positions.copyWithin(after + delta, after, count);
      after += delta;
      count += delta;
      for (let j = index + 1; j < after; j++) positions[j] = positions[j - 1]! + 1;
      for (; after < count; after++) positions[after]! += delta;
    }
    return end;
  }

  let maxOps = 0;

  /** Applies the first subtable of a lookup that applies at `at`; returns the position after, or -1. */
  function applyLookupAt(id: number, at: number, depth: number): number {
    const lookup = lookups[id]!;
    lookupId = id;
    lookupFlag = data[lookup]!;
    markSet = data[lookup + 1]!;
    for (let s = 0; s < data[lookup + 3]!; s++) {
      const after = applySubtable(data[lookup + 4 + s]!, at, depth);
      if (after >= 0) return after;
    }
    return -1;
  }

  function replace(at: number, glyph: number) {
    if (glyphs[at] === glyph) return;
    changes++;
    glyphs[at] = glyph;
    mark(at);
  }

  function applySubtable(subtable: number, at: number, depth: number): number {
    const glyph = glyphs[at]!;
    const kind = data[subtable]!;
    if (kind === KIND.coverageRules) {
      const inputs = subtable + 2 + data[subtable + 1]!;
      if (data[inputs] === 0 || coverageIndex(data[inputs + 1]!, glyph) < 0) return -1;
      // The rule is the subtable itself, from its backtrack count, the input with the first.
      return applyRule(at, subtable + 1, BY_COVERAGE, 0, 1, depth);
    }
    const index = coverageIndex(data[subtable + 1]!, glyph);
    if (index < 0) return -1;
    switch (kind) {
      case KIND.single: {
        if (index >= data[subtable + 2]! || data[subtable + 3 + index] === NONE) return -1;
        const start = starts[at]!;
        replace(at, data[subtable + 3 + index]!);
        if (depth === 0) addSpan(start, ends[at]!, glyph !== glyphs[at]);
        return at + 1;
      }
      case KIND.multiple: {
        if (index >= data[subtable + 2]!) return -1;
        const sequence = data[subtable + 3 + index]!;
        const count = data[sequence]!;
        const [start, end] = [starts[at]!, ends[at]!];
        // One glyph for one moves none.
        if (count === 1) {
          replace(at, data[sequence + 1]!);
          if (depth === 0) addSpan(start, end, glyph !== glyphs[at]);
          return at + 1;
        }
        changes++;
        if (depth === 0) addSpan(start, end, true);
        moved = true;
        if (count === 0) {
          remove(at);
          return at;
        }
        insertAfter(at, count - 1);
        for (let k = 0; k < count; k++) {
          glyphs[at + k] = data[sequence + 1 + k]!;
          starts[at + k] = start;
          ends[at + k] = end;
          flags[at + k] = flags[at]!;
        }
        return at + count;
      }
      case KIND.ligature: {
        if (index >= data[subtable + 2]!) return -1;
        const set = data[subtable + 3 + index]!;
        for (let l = 0; l < data[set]!; l++) {
          const ligature = data[set + 1 + l]!;
          const components = data[ligature + 1]!;
          if (input.length < components + 1) input = new Int32Array(components * 2 + 2);
          input[0] = at;
          let k = 1;
          for (; k <= components; k++) {
            const found = next(input[k - 1]!, 1, false, BY_GLYPH, data[ligature + 1 + k]!);
            if (found < 0) break;
            input[k] = found;
          }
          if (k <= components) continue;
          const last = input[components]!;
          let start = starts[at]!;
          let end = ends[at]!;
          for (let i = at + 1; i <= last; i++) {
            start = Math.min(start, starts[i]!);
            end = Math.max(end, ends[i]!);
          }
          for (let i = at; i <= last; i++) [starts[i], ends[i]] = [start, end];
          changes++;
          moved = true;
          glyphs[at] = data[ligature]!;
          for (let c = components; c >= 1; c--) remove(input[c]!);
          // Also when a rule applied it: its components may reach past the rule's context.
          addSpan(start, end, true);
          return last - components + 1;
        }
        return -1;
      }
      case KIND.glyphRules:
      case KIND.classRules: {
        const byClass = kind === KIND.classRules;
        const sets = byClass ? subtable + 5 : subtable + 2;
        const which = byClass ? classOf(data[subtable + 3]!, glyph) : index;
        if (which >= data[sets]!) return -1;
        const set = data[sets + 1 + which]!;
        for (let r = 0; r < data[set]!; r++) {
          const after = applyRule(at, data[set + 1 + r]!, byClass ? BY_CLASS : BY_GLYPH, subtable + 2, 0, depth);
          if (after >= 0) return after;
        }
        return -1;
      }
      case KIND.reverse: {
        // Only applied on its own, from the end of the text (HarfBuzz doesn't chain to it).
        if (depth > 0) return -1;
        let from = at;
        for (let k = 0; k < data[subtable + 2]!; k++) {
          from = next(from, -1, true, BY_COVERAGE, data[subtable + 3 + k]!);
          if (from < 0) return -1;
        }
        const lookahead = subtable + 3 + data[subtable + 2]!;
        let to = at;
        for (let k = 0; k < data[lookahead]!; k++) {
          to = next(to, 1, true, BY_COVERAGE, data[lookahead + 1 + k]!);
          if (to < 0) return -1;
        }
        const substitutes = lookahead + 1 + data[lookahead]!;
        if (index >= data[substitutes]!) return -1;
        let start = starts[from]!;
        let end = ends[from]!;
        for (let i = from + 1; i <= to; i++) {
          start = Math.min(start, starts[i]!);
          end = Math.max(end, ends[i]!);
        }
        replace(at, data[substitutes + 1 + index]!);
        addSpan(start, end, glyph !== glyphs[at]);
        return at;
      }
    }
    return -1;
  }

  /** Notes the lookups after the current one each glyph may start; whether there are any. */
  function markAll(): boolean {
    for (let i = 0; i < size; i++) mark(i);
    return lastAt.some((last) => last >= 0);
  }

  function startsAt(k: number, glyph: number): boolean {
    return (startBits[k * words + (glyph >> 5)]! & (1 << (glyph & 31))) !== 0;
  }

  /** Shapes `text` into the buffer; false if no lookup could apply to it. */
  function run(text: string): boolean {
    size = 0;
    spanCount = 0;
    reserve(text.length);
    for (let i = 0; i < text.length;) {
      let c = text.charCodeAt(i);
      let length = 1;
      if (c >= 0xd800 && c < 0xe000) {
        const low = text.charCodeAt(i + 1);
        if (c < 0xdc00 && low >= 0xdc00 && low < 0xe000) {
          c = ((c - 0xd800) << 10) + (low - 0xdc00) + 0x10000;
          length = 2;
        } else {
          c = 0xfffd;
        }
      }
      glyphs[size] = c < 128 ? ascii[c]! : glyphOf(c);
      starts[size] = i;
      ends[size] = i + length;
      flags[size++] = characterFlags(c);
      i += length;
    }
    firstAt.fill(0x7fffffff);
    lastAt.fill(-1);
    current = -1;
    if (!markAll()) return false;
    maxOps = Math.max(MIN_OPS, size * MAX_OPS_FACTOR);
    ops = 0;
    for (let k = 0; k < applied.length; k++) {
      if (lastAt[k]! < firstAt[k]!) continue;
      const id = applied[k]!;
      current = k;
      moved = false;
      lookupId = id;
      lookupFlag = data[lookups[id]!]!;
      markSet = data[lookups[id]! + 1]!;
      if (data[lookups[id]! + 2]!) {
        for (let i = lastAt[k]!; i >= firstAt[k]!; i--) {
          if (startsAt(k, glyphs[i]!) && looksAt(glyphs[i]!)) applyLookupAt(id, i, 0);
        }
      } else {
        // The last position moves with the glyphs added or removed before it.
        const last = lastAt[k]! - size;
        for (let i = firstAt[k]!; i < size && i <= last + size;) {
          if (startsAt(k, glyphs[i]!) && looksAt(glyphs[i]!)) {
            const after = applyLookupAt(id, i, 0);
            if (after >= 0) {
              i = after;
              continue;
            }
          }
          i++;
        }
      }
      // Positions noted for later lookups may have moved: look at the whole text for them.
      if (moved) {
        for (let later = k + 1; later < applied.length; later++) {
          if (lastAt[later]! >= 0) [firstAt[later], lastAt[later]] = [0, size - 1];
        }
        markAll();
      }
    }
    return true;
  }

  return {
    ranges(text) {
      if (applied.length === 0 || !run(text) || spanCount === 0) return [];
      return joined(text);
    },
    shape(text) {
      run(text);
      return { glyphs: Array.from(glyphs.subarray(0, size)), clusters: Array.from(starts.subarray(0, size)) };
    },
  };

  /** The ranges the spans found give (see the top of the file). */
  function joined(text: string): [number, number][] {
    const ranges: [number, number][] = [];
    for (let s = 0; s < spanCount; s++) {
      const [start, end] = [spanStarts[s]!, spanEnds[s]!];
      // More than one character: one alone is drawn as it is.
      const pair = end - start === 2 && text.charCodeAt(start) >= 0xd800 && text.charCodeAt(start) < 0xdc00;
      if (spanChanged[s] && end - start > 1 && !pair) ranges.push([start, end]);
    }
    if (ranges.length === 0) return ranges;
    for (let widened = true; widened;) {
      widened = false;
      merge(ranges);
      for (let s = 0; s < spanCount; s++) {
        const [start, end] = [spanStarts[s]!, spanEnds[s]!];
        for (const range of ranges) {
          if (start < range[1] && range[0] < end && (start < range[0] || end > range[1])) {
            range[0] = Math.min(range[0], start);
            range[1] = Math.max(range[1], end);
            widened = true;
          }
        }
      }
    }
    return ranges;
  }
}

/** Sorts ranges and merges those that overlap, in place; ranges that only touch stay apart. */
export function merge(ranges: [number, number][]): void {
  ranges.sort((a, b) => a[0] - b[0]);
  let kept = 0;
  for (const range of ranges) {
    const last = ranges[kept - 1];
    if (last && range[0] < last[1]) last[1] = Math.max(last[1], range[1]);
    else ranges[kept++] = range;
  }
  ranges.length = kept;
}

/**
 * `ranges` with the results for the texts looked up last kept, up to `max` characters of text
 * in all. Each call gets ranges of its own: xterm.js turns them into cells in place.
 */
export function cachedRanges(ranges: (text: string) => [number, number][], max: number): (text: string) => [number, number][] {
  const cache = new Map<string, [number, number][]>();
  let size = 0;
  return (text) => {
    let found = cache.get(text);
    if (found) {
      // The most recently used last.
      cache.delete(text);
      cache.set(text, found);
    } else {
      found = ranges(text);
      if (text.length <= max) {
        cache.set(text, found);
        size += text.length;
        for (const oldest of cache.keys()) {
          if (size <= max) break;
          cache.delete(oldest);
          size -= oldest.length;
        }
      }
    }
    return found.map(([start, end]) => [start, end]);
  };
}
