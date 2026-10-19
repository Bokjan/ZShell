import { describe, expect, it } from "vitest";

import { LIGATURE_SEQUENCES, ligatureRanges } from "./ligatures";

describe("ligatureRanges", () => {
  it("finds each sequence on its own", () => {
    for (const sequence of LIGATURE_SEQUENCES) expect(ligatureRanges(`a${sequence}b`)).toEqual([[1, 1 + sequence.length]]);
  });

  it("takes the longest sequence at each place, from the left", () => {
    expect(ligatureRanges("a !== b")).toEqual([[2, 5]]);
    expect(ligatureRanges("<!----")).toEqual([[0, 5]]);
    expect(ligatureRanges("=====")).toEqual([
      [0, 3],
      [3, 5],
    ]);
    expect(ligatureRanges("x->y<=z")).toEqual([
      [1, 3],
      [4, 6],
    ]);
  });

  it("finds nothing in text without the sequences", () => {
    expect(ligatureRanges("")).toEqual([]);
    expect(ligatureRanges("-rw-r--r-- 1 root root 4096 a.txt")).toEqual([]);
    expect(ligatureRanges("编译完成 - ok")).toEqual([]);
  });

  it("counts offsets in UTF-16 code units, as xterm.js does", () => {
    expect(ligatureRanges("编译 => 😀 != x")).toEqual([
      [3, 5],
      [9, 11],
    ]);
  });
});
