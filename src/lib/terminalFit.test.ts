import { describe, expect, it } from "vitest";

import { fitSize } from "./terminalFit";

describe("fitSize", () => {
  it("fits whole cells across the whole space", () => {
    expect(fitSize({ width: 800, height: 600 }, { width: 8, height: 17 })).toEqual({ cols: 100, rows: 35 });
    expect(fitSize({ width: 807.9, height: 16.9 }, { width: 8, height: 17 })).toEqual({ cols: 100, rows: 1 });
  });

  it("keeps the smallest terminal in a space too small or collapsed", () => {
    expect(fitSize({ width: 0, height: 0 }, { width: 8, height: 17 })).toEqual({ cols: 2, rows: 1 });
    expect(fitSize({ width: -20, height: -5 }, { width: 8, height: 17 })).toEqual({ cols: 2, rows: 1 });
  });

  it("gives nothing before the cells are measured", () => {
    expect(fitSize({ width: 800, height: 600 }, { width: 0, height: 17 })).toBeUndefined();
    expect(fitSize({ width: 800, height: 600 }, { width: 8, height: 0 })).toBeUndefined();
  });
});
