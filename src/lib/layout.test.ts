import { describe, expect, it } from "vitest";

import {
  autoSide,
  canSplit,
  dividers,
  fits,
  halfOn,
  heirOf,
  insertLayout,
  moveDivider,
  neighbor,
  paneKeys,
  paneRects,
  removeFromLayout,
  sideAt,
  splitLayout,
} from "./layout";
import type { Layout } from "./panes";

const pane = (key: number): Layout => ({ kind: "pane", key });

// 1 | 2
//   | 3   (2 above 3, the right half split in two)
const threePanes = splitLayout(splitLayout(pane(1), 1, "row", 2), 2, "column", 3);

describe("splitting and removing", () => {
  it("adds a pane next to the one split, sharing its space", () => {
    expect(paneKeys(threePanes)).toEqual([1, 2, 3]);
    // Splitting again the same way adds to the split rather than nesting.
    const four = splitLayout(threePanes, 1, "row", 4);
    expect(four.kind === "split" && four.children.length).toBe(3);
    expect(four.kind === "split" && four.sizes).toEqual([0.25, 0.25, 0.5]);
  });

  it("gives a removed pane's space to its neighbor, and merges what is left", () => {
    // Removing 2 leaves 1 | 3.
    expect(removeFromLayout(threePanes, 2)).toEqual({ kind: "split", direction: "row", children: [pane(1), pane(3)], sizes: [0.5, 0.5] });
    expect(heirOf(threePanes, 2)).toBe(3);
    expect(heirOf(threePanes, 1)).toBe(2);
    // A split of the same direction left inside another merges into it.
    const split = (direction: "row" | "column", ...children: Layout[]): Layout => ({
      kind: "split",
      direction,
      children,
      sizes: children.map(() => 1 / children.length),
    });
    // 1 | 2 above (3 | 4)
    const nested = split("row", pane(1), split("column", pane(2), split("row", pane(3), pane(4))));
    const merged = removeFromLayout(nested, 2);
    expect(merged).toEqual({ kind: "split", direction: "row", children: [pane(1), pane(3), pane(4)], sizes: [0.5, 0.25, 0.25] });
    expect(removeFromLayout(pane(1), 1)).toBeNull();
  });
});

describe("moving between panes", () => {
  it("finds the pane beyond each edge", () => {
    expect(neighbor(threePanes, 1, "right")).toBe(2);
    expect(neighbor(threePanes, 3, "left")).toBe(1);
    expect(neighbor(threePanes, 2, "down")).toBe(3);
    expect(neighbor(threePanes, 3, "up")).toBe(2);
    expect(neighbor(threePanes, 1, "left")).toBeNull();
  });
});

describe("dividers", () => {
  it("keep the panes beside them at least the minimum size", () => {
    const [divider] = dividers(threePanes);
    const moved = (at: number) => {
      const layout = moveDivider(threePanes, divider, at, 0.1);
      return layout.kind === "split" ? layout.sizes : null;
    };
    expect(moved(0.3)).toEqual([0.3, 0.7]);
    expect(moved(0.02)).toEqual([0.1, 0.9]);
    expect(moved(0.99)?.[0]).toBeCloseTo(0.9);
  });
});

describe("canSplit", () => {
  it("allows a split that leaves each half at least the minimum size", () => {
    const area = { width: 600, height: 200 };
    // Pane 2 is 300 × 100: room for two 150-wide panes, not for two 50-high ones.
    expect(canSplit(threePanes, 2, area, "row")).toBe(true);
    expect(canSplit(threePanes, 2, area, "column")).toBe(false);
    expect(canSplit(threePanes, 2, { width: 200, height: 400 }, "row")).toBe(false);
    // Not measured yet: allowed, as the menus can't tell.
    expect(canSplit(threePanes, 2, undefined, "column")).toBe(true);
  });
});

describe("insertLayout", () => {
  it("puts a pane on any side of another, before it on the left and above", () => {
    expect(insertLayout(pane(1), 1, "left", pane(2))).toEqual({
      kind: "split",
      direction: "row",
      children: [pane(2), pane(1)],
      sizes: [0.5, 0.5],
    });
    expect(insertLayout(pane(1), 1, "down", pane(2))).toEqual({
      kind: "split",
      direction: "column",
      children: [pane(1), pane(2)],
      sizes: [0.5, 0.5],
    });
    // Above 2, in the right half: 2's space is shared, the rest is as it was.
    const above = insertLayout(threePanes, 2, "up", pane(4));
    expect(paneKeys(above)).toEqual([1, 4, 2, 3]);
    expect(paneRects(above).get(4)).toEqual({ x: 0.5, y: 0, w: 0.5, h: 0.25 });
    expect(paneRects(above).get(3)).toEqual({ x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
  });

  it("brings a tab's panes split the same way in as siblings, in their proportions", () => {
    const tab: Layout = { kind: "split", direction: "row", children: [pane(4), pane(5)], sizes: [0.25, 0.75] };
    const merged = insertLayout(threePanes, 1, "left", tab);
    expect(merged).toMatchObject({ kind: "split", direction: "row", sizes: [0.0625, 0.1875, 0.25, 0.5] });
    expect(paneKeys(merged)).toEqual([4, 5, 1, 2, 3]);
    // Split the other way, they stay together.
    const nested = insertLayout(threePanes, 1, "up", tab);
    expect(paneRects(nested).get(5)).toEqual({ x: 0.125, y: 0, w: 0.375, h: 0.5 });
  });

  it("splits as splitLayout did", () => {
    expect(splitLayout(threePanes, 3, "column", 4)).toEqual(insertLayout(threePanes, 3, "down", pane(4)));
  });
});

describe("placing a pane", () => {
  it("goes right of a wide pane and below a tall one, or where there is room", () => {
    const area = { width: 600, height: 200 };
    // Pane 1 is 300 × 200, pane 2 300 × 100.
    expect(autoSide(threePanes, 1, area)).toBe("right");
    expect(autoSide(threePanes, 1, { width: 300, height: 800 })).toBe("down");
    // 150 × 400 has no room across: below it.
    expect(autoSide(threePanes, 1, { width: 300, height: 400 })).toBe("down");
    expect(autoSide(threePanes, 2, { width: 200, height: 200 })).toBeNull();
    expect(autoSide(threePanes, 9, area)).toBeNull();
  });

  it("checks that panes are at least the minimum size", () => {
    expect(fits(threePanes, [2, 3], { width: 600, height: 200 })).toBe(true);
    expect(fits(threePanes, [2, 3], { width: 600, height: 100 })).toBe(false);
    expect(fits(threePanes, [1], undefined)).toBe(true);
  });

  it("finds the side of a pane nearest a point, cutting it along its diagonals", () => {
    const rect = { x: 100, y: 100, w: 200, h: 100 };
    expect(sideAt(rect, 110, 150)).toBe("left");
    expect(sideAt(rect, 290, 150)).toBe("right");
    expect(sideAt(rect, 200, 105)).toBe("up");
    expect(sideAt(rect, 200, 195)).toBe("down");
    expect(halfOn(rect, "right")).toEqual({ x: 200, y: 100, w: 100, h: 100 });
    expect(halfOn(rect, "up")).toEqual({ x: 100, y: 100, w: 200, h: 50 });
  });
});
