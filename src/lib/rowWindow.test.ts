import { describe, expect, it } from "vitest";

import { rowSegments } from "./rowWindow";

describe("rowSegments", () => {
  it("draws the rows in view, with gaps for the others", () => {
    expect(rowSegments(100, 10, 20, [])).toEqual([
      { kind: "gap", rows: 10 },
      { kind: "rows", start: 10, end: 20 },
      { kind: "gap", rows: 80 },
    ]);
    expect(rowSegments(5, -3, 9, [])).toEqual([{ kind: "rows", start: 0, end: 5 }]);
  });

  it("also draws the rows kept, merged with those next to them", () => {
    expect(rowSegments(100, 10, 20, [50, 20, 99, -1, 100])).toEqual([
      { kind: "gap", rows: 10 },
      { kind: "rows", start: 10, end: 21 },
      { kind: "gap", rows: 29 },
      { kind: "rows", start: 50, end: 51 },
      { kind: "gap", rows: 48 },
      { kind: "rows", start: 99, end: 100 },
    ]);
    expect(rowSegments(10, 0, 0, [3])).toEqual([
      { kind: "gap", rows: 3 },
      { kind: "rows", start: 3, end: 4 },
      { kind: "gap", rows: 6 },
    ]);
  });
});
