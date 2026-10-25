import { describe, expect, it } from "vitest";

import { DEBOUNCE_COLS_FROM, fitSize, resizeStep } from "./terminalFit";

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

describe("resizeStep", () => {
  const size = { cols: 80, rows: 24 };
  const long = DEBOUNCE_COLS_FROM;

  it("takes a new size at once while the buffer is short", () => {
    expect(resizeStep(size, { cols: 100, rows: 30 }, long - 1)).toEqual({ now: { cols: 100, rows: 30 }, colsLater: false });
  });

  it("takes new rows at once and leaves new columns for later in a long buffer", () => {
    expect(resizeStep(size, { cols: 100, rows: 30 }, long)).toEqual({ now: { cols: 80, rows: 30 }, colsLater: true });
    expect(resizeStep(size, { cols: 100, rows: 24 }, long)).toEqual({ now: null, colsLater: true });
    expect(resizeStep(size, { cols: 80, rows: 30 }, long)).toEqual({ now: { cols: 80, rows: 30 }, colsLater: false });
  });

  it("does nothing when the size stays", () => {
    expect(resizeStep(size, size, long)).toEqual({ now: null, colsLater: false });
  });
});
