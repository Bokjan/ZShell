import type { KeyboardEvent } from "react";
import { describe, expect, it, vi } from "vitest";

import { clampIndex, continuesTypeAhead, navigateList, rovingTarget, TYPEAHEAD_MS, typeAhead } from "./listNavigation";

const key = (name: string) =>
  ({ key: name, keyCode: 0, nativeEvent: { isComposing: false }, preventDefault: vi.fn() }) as unknown as KeyboardEvent;

describe("clampIndex", () => {
  it("keeps the index within the list, or -1 for an empty one", () => {
    expect(clampIndex(5, 3)).toBe(2);
    expect(clampIndex(-1, 3)).toBe(0);
    expect(clampIndex(1, 3)).toBe(1);
    expect(clampIndex(0, 0)).toBe(-1);
  });
});

describe("navigateList", () => {
  it("moves within the list", () => {
    const move = vi.fn();
    expect(navigateList(key("ArrowDown"), 3, 2, move)).toBe(true);
    expect(navigateList(key("ArrowUp"), 3, 0, move)).toBe(true);
    expect(navigateList(key("ArrowDown"), 3, 0, move)).toBe(true);
    expect(move.mock.calls).toEqual([[2], [0], [1]]);
  });

  it("picks only an item that is there", () => {
    const pick = vi.fn();
    expect(navigateList(key("Enter"), 0, -1, vi.fn(), pick)).toBe(false);
    expect(navigateList(key("Enter"), 2, 1, vi.fn(), pick)).toBe(true);
    expect(pick.mock.calls).toEqual([[1]]);
  });

  it("does nothing in an empty list, and leaves other keys alone", () => {
    const move = vi.fn();
    const down = key("ArrowDown");
    expect(navigateList(down, 0, -1, move)).toBe(true);
    expect(move).not.toHaveBeenCalled();
    const other = key("a");
    expect(navigateList(other, 3, 0, move)).toBe(false);
    expect(other.preventDefault).not.toHaveBeenCalled();
  });
});

describe("rovingTarget", () => {
  it("moves with the arrow keys, wrapping around, and to the ends", () => {
    expect(rovingTarget("ArrowRight", 0, 3)).toBe(1);
    expect(rovingTarget("ArrowDown", 2, 3)).toBe(0);
    expect(rovingTarget("ArrowLeft", 0, 3)).toBe(2);
    expect(rovingTarget("ArrowUp", 1, 3)).toBe(0);
    expect(rovingTarget("Home", 2, 3)).toBe(0);
    expect(rovingTarget("End", 0, 3)).toBe(2);
  });

  it("leaves ↑ / ↓ and other keys alone when asked to", () => {
    expect(rovingTarget("ArrowDown", 0, 3, true)).toBeNull();
    expect(rovingTarget("Enter", 0, 3)).toBeNull();
  });
});

describe("typeAhead", () => {
  const labels = ["Apple", "Banana", null, "Blueberry", "Cherry"];
  const fresh = { text: "", at: 0 };

  it("finds the next item that starts with a letter, after the highlighted one", () => {
    expect(typeAhead(fresh, "b", 10_000, labels, -1).index).toBe(1);
    expect(typeAhead(fresh, "B", 10_000, labels, 1).index).toBe(3);
    // Around to the start.
    expect(typeAhead(fresh, "b", 10_000, labels, 3).index).toBe(1);
    expect(typeAhead(fresh, "z", 10_000, labels, 0).index).toBe(-1);
  });

  it("keeps the highlighted item while the prefix grows, and starts over after a pause", () => {
    const first = typeAhead(fresh, "b", 10_000, labels, -1);
    expect(first.typed).toEqual({ text: "b", at: 10_000 });
    const second = typeAhead(first.typed, "l", 10_100, labels, first.index);
    expect(second).toEqual({ typed: { text: "bl", at: 10_100 }, index: 3 });
    expect(typeAhead(first.typed, "l", 10_000 + TYPEAHEAD_MS, labels, first.index).typed.text).toBe("l");
  });

  it("goes through the items that start with a letter typed again and again", () => {
    const first = typeAhead(fresh, "b", 10_000, labels, 0);
    const second = typeAhead(first.typed, "b", 10_100, labels, first.index);
    expect([first.index, second.index]).toEqual([1, 3]);
    expect(typeAhead(second.typed, "b", 10_200, labels, second.index)).toEqual({ typed: { text: "bbb", at: 10_200 }, index: 1 });
  });

  it("skips items that can't be chosen", () => {
    expect(typeAhead(fresh, "x", 10_000, ["x", null], 0).index).toBe(0);
    expect(typeAhead(fresh, "x", 10_000, [null, null], -1).index).toBe(-1);
  });
});

describe("continuesTypeAhead", () => {
  it("is only while letters are being typed", () => {
    expect(continuesTypeAhead({ text: "", at: 10_000 }, 10_100)).toBe(false);
    expect(continuesTypeAhead({ text: "b", at: 10_000 }, 10_100)).toBe(true);
    expect(continuesTypeAhead({ text: "b", at: 10_000 }, 10_000 + TYPEAHEAD_MS)).toBe(false);
  });
});
