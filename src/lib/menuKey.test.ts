import { describe, expect, it, vi } from "vitest";

import { followsMenuKey, isMenuKey, openedByMenuKey } from "./menuKey";

describe("isMenuKey", () => {
  it("is the menu key, or Shift+F10", () => {
    expect(isMenuKey({ key: "ContextMenu", shiftKey: false })).toBe(true);
    expect(isMenuKey({ key: "F10", shiftKey: true })).toBe(true);
    expect(isMenuKey({ key: "F10", shiftKey: false })).toBe(false);
    expect(isMenuKey({ key: "Enter", shiftKey: true })).toBe(false);
  });
});

describe("followsMenuKey", () => {
  const event = () => ({ preventDefault: vi.fn(), stopPropagation: vi.fn() });

  it("cancels the contextmenu event that comes right after a menu key opened a menu", () => {
    vi.useFakeTimers();
    try {
      // None opened yet.
      const before = event();
      expect(followsMenuKey(before)).toBe(false);
      expect(before.preventDefault).not.toHaveBeenCalled();

      openedByMenuKey();
      const after = event();
      expect(followsMenuKey(after)).toBe(true);
      expect(after.preventDefault).toHaveBeenCalled();
      expect(after.stopPropagation).toHaveBeenCalled();

      // Later, a right-click opens its own menu.
      vi.advanceTimersByTime(600);
      expect(followsMenuKey(event())).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});
