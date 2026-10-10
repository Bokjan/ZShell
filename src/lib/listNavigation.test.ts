import type { KeyboardEvent } from "react";
import { describe, expect, it, vi } from "vitest";

import { clampIndex, navigateList } from "./listNavigation";

const key = (name: string) => ({ key: name, keyCode: 0, nativeEvent: { isComposing: false }, preventDefault: vi.fn() }) as unknown as KeyboardEvent;

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
