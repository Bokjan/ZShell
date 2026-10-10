import { describe, expect, it } from "vitest";

import { canSplit, dividers, heirOf, moveDivider, neighbor, paneKeys, removeFromLayout, splitLayout } from "./layout";
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
