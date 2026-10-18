import { describe, expect, it } from "vitest";

import { dragSlot } from "./tabStrip";

describe("dragSlot", () => {
  // Three tabs 100 wide, the dragged one (100 wide) taken out from among them.
  const others = [0, 100, 200].map((left, i) => ({ key: i + 1, left, width: 100 }));

  it("goes past the tabs whose middle is before the pointer", () => {
    expect(dragSlot(others, 100, 0, 10)).toEqual({ index: 0, resting: null });
    // Past tab 1, shown at 100–200 while the dragged one is before it.
    expect(dragSlot(others, 100, 0, 190)).toEqual({ index: 1, resting: null });
    // Back over tab 1, now shown at 0–100: it stays past it until the pointer is before its middle.
    expect(dragSlot(others, 100, 1, 90)).toEqual({ index: 1, resting: null });
    expect(dragSlot(others, 100, 1, 10)).toEqual({ index: 0, resting: null });
    expect(dragSlot(others, 100, 1, 390)).toEqual({ index: 3, resting: null });
  });

  it("rests on the middle half of a tab, where it is shown", () => {
    expect(dragSlot(others, 100, 0, 150)).toEqual({ index: 0, resting: 1 });
    expect(dragSlot(others, 100, 1, 50)).toEqual({ index: 1, resting: 1 });
    expect(dragSlot(others, 100, 1, 250)).toEqual({ index: 1, resting: 2 });
  });
});
