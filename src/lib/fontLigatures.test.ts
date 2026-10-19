import { describe, expect, it } from "vitest";

import type { LigatureRules } from "../ipc/bindings";
import { cachedRanges, characterFlags, fontLigatures, merge } from "./fontLigatures";
// The rules of a font made up for the backend's tests (`fonts/rules.rs`, which says what it
// substitutes), and the glyph id of each number.
import fixture from "./fontLigatures.fixture.json";

const font = fontLigatures(fixture.rules as LigatureRules);

/** The glyph ids `text` ends up with, -1 for those the rules don't tell apart. */
function glyphs(text: string): number[] {
  return font.shape(text).glyphs.map((number) => fixture.glyphs[number] ?? -1);
}

describe("fontLigatures", () => {
  it("joins what a rule changed with its context", () => {
    expect(font.ranges("x->y")).toEqual([[1, 3]]);
    // The arrow's start is only substituted; its end is matched by another rule.
    expect(glyphs("x->y")).toEqual([-1, -1, 12, -1]);
    expect(font.ranges("a <= b")).toEqual([[2, 4]]);
    expect(font.ranges("aaw")).toEqual([[1, 3]]);
    expect(glyphs("aaw")).toEqual([7, -1, 10]);
  });

  it("joins nothing where a rule keeps the others from applying", () => {
    expect(font.ranges("-->")).toEqual([]);
    expect(glyphs("-->")).toEqual([1, 1, 2]);
  });

  it("widens a range to the context of rules found over part of it", () => {
    // The arrow, then a rule on its end and the `=` after it that changes nothing.
    expect(font.ranges("->=")).toEqual([[0, 3]]);
    expect(font.ranges("-> =")).toEqual([[0, 2]]);
  });

  it("ligates past the glyphs a lookup skips, keeping them after the ligature", () => {
    expect(font.ranges("fi")).toEqual([[0, 2]]);
    expect(font.shape("f́i")).toEqual({ glyphs: [font.shape("fi").glyphs[0], fixture.glyphs.indexOf(8)], clusters: [0, 0] });
    expect(font.ranges("f́i")).toEqual([[0, 3]]);
  });

  it("gives each glyph of a multiple substitution the character's place, and joins nothing for one character", () => {
    expect(font.shape("aq")).toEqual({
      glyphs: [fixture.glyphs.indexOf(7), fixture.glyphs.indexOf(16), fixture.rules.glyphCount],
      clusters: [0, 1, 1],
    });
    expect(font.ranges("q")).toEqual([]);
  });

  it("counts offsets in UTF-16 code units", () => {
    expect(font.ranges("\u{1f680}->")).toEqual([[2, 4]]);
    expect(font.ranges("\u{1f680}\u{1f680} <=")).toEqual([[5, 7]]);
  });

  it("skips zero-width joiners in the input, but not non-joiners", () => {
    expect(font.ranges("-‍>")).toEqual([[0, 3]]);
    expect(font.ranges("-‌>")).toEqual([]);
  });

  it("finds nothing in a font without rules", () => {
    const none = fontLigatures({ ...(fixture.rules as LigatureRules), applied: [] });
    expect(none.ranges("->")).toEqual([]);
  });

  it("gives new ranges each time", () => {
    const first = font.ranges("->");
    first[0]![0] = 5;
    expect(font.ranges("->")).toEqual([[0, 2]]);
  });
});

describe("characterFlags", () => {
  it("tells the joiners and the default ignorables HarfBuzz skips", () => {
    expect([0x41, 0x200d, 0x200c, 0xfe0f, 0xad].map(characterFlags)).toEqual([0, 3, 5, 1, 1]);
    // The combining grapheme joiner and tags stay in the way.
    expect([0x34f, 0xe0041, 0xe0100].map(characterFlags)).toEqual([0, 0, 1]);
  });
});

describe("merge", () => {
  it("sorts and merges ranges that overlap, keeping those that only touch apart", () => {
    const ranges: [number, number][] = [
      [5, 7],
      [0, 2],
      [1, 3],
      [3, 4],
      [6, 9],
    ];
    merge(ranges);
    expect(ranges).toEqual([
      [0, 3],
      [3, 4],
      [5, 9],
    ]);
  });
});

describe("cachedRanges", () => {
  it("finds a text again from the cache, giving new ranges each time", () => {
    let calls = 0;
    const find = cachedRanges((text) => (calls++, [[0, text.length]]), 10);
    const first = find("abc");
    first[0]![1] = 99;
    expect(find("abc")).toEqual([[0, 3]]);
    expect(calls).toBe(1);
  });

  it("keeps the texts used last, up to the size in characters", () => {
    let calls = 0;
    const find = cachedRanges(() => (calls++, []), 6);
    find("aaa");
    find("bbb");
    find("aaa");
    // Pushes out "bbb", the least recently used.
    find("ccc");
    find("aaa");
    expect(calls).toBe(3);
    find("bbb");
    expect(calls).toBe(4);
    // Too long to keep.
    find("0123456789");
    find("0123456789");
    expect(calls).toBe(6);
  });
});
