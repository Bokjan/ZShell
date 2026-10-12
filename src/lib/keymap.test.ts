import { describe, expect, it, vi } from "vitest";

import { actionEvent, actionOf, appMenu, chosenAction, labelsOn, matches, paneSideOf, shortcutRows, tabIndexOf } from "./keymap";

const key = (code: string, modifiers: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean } = {}) =>
  ({
    code,
    ctrlKey: !!modifiers.ctrl,
    altKey: !!modifiers.alt,
    shiftKey: !!modifiers.shift,
    metaKey: !!modifiers.meta,
  }) as KeyboardEvent;

// The tests run outside macOS (no "Mac" in the user agent), so the other platforms' keys apply.
describe("keymap", () => {
  it("matches exactly the modifiers of a shortcut", () => {
    expect(matches(key("KeyF", { ctrl: true, shift: true }), "find")).toBe(true);
    expect(matches(key("KeyF", { ctrl: true }), "find")).toBe(false);
    expect(matches(key("KeyF", { ctrl: true, shift: true, alt: true }), "find")).toBe(false);
  });

  it("matches every key of an action and the keypad's like the main keyboard's", () => {
    expect(matches(key("Insert", { shift: true }), "paste")).toBe(true);
    expect(matches(key("KeyV", { ctrl: true }), "paste")).toBe(true);
    expect(matches(key("NumpadAdd", { alt: true, shift: true }), "splitRight")).toBe(true);
    expect(actionOf(key("Minus", { alt: true, shift: true }), ["splitRight", "splitDown"])).toBe("splitDown");
  });

  it("finds tabs and panes", () => {
    expect(tabIndexOf(key("Digit3", { alt: true }))).toBe(2);
    expect(tabIndexOf(key("Digit9", { alt: true }))).toBe(-1);
    expect(tabIndexOf(key("Digit3", { ctrl: true }))).toBe(null);
    expect(paneSideOf(key("ArrowUp", { ctrl: true, alt: true }))).toBe("up");
    expect(paneSideOf(key("ArrowUp", { alt: true }))).toBe(null);
  });

  it("labels keys as each platform writes them", () => {
    expect(labelsOn("filePanel", true)).toEqual(["⇧⌘E"]);
    expect(labelsOn("filePanel", false)).toEqual(["Ctrl+Shift+E"]);
    expect(labelsOn("focusLeft", true)).toEqual(["⌥⌘←"]);
    expect(labelsOn("splitRight", false)).toEqual(["Alt+Shift+="]);
    expect(labelsOn("settings", true)).toEqual(["⌘,"]);
    expect(labelsOn("copy", false)).toEqual(["Ctrl+Shift+C", "Ctrl+Insert"]);
    expect(labelsOn("closeWindow", false)).toEqual([]);
  });

  it("lists the shortcuts of each platform", () => {
    const keys = (mac: boolean) => Object.fromEntries(shortcutRows(mac).map((row) => [row.id, row.keys]));
    expect(keys(true)).toMatchObject({ goToTab: "⌘1 – ⌘8", focusPane: "⌥⌘ ← ↑ → ↓", closeWindow: "⇧⌘W" });
    expect(keys(false)).toMatchObject({ goToTab: "Alt+1 – Alt+8", focusPane: "Ctrl+Alt+← ↑ → ↓", copySelection: "Ctrl+C" });
    expect(keys(true)).not.toHaveProperty("copySelection");
    expect(keys(false)).not.toHaveProperty("closeWindow");
  });

  it("takes an action chosen from a menu for its keys, and only its", () => {
    // Node has no key events.
    vi.stubGlobal("KeyboardEvent", class extends Event {});
    const e = actionEvent("splitRight");
    expect(matches(e, "splitRight")).toBe(true);
    expect(matches(e, "splitDown")).toBe(false);
    expect(actionOf(e, ["splitDown", "splitRight"])).toBe("splitRight");
    expect(tabIndexOf(e)).toBe(null);
    expect(chosenAction(e)).toBe("splitRight");
    expect(chosenAction(key("KeyF", { ctrl: true, shift: true }))).toBe(null);
    vi.unstubAllGlobals();
  });

  it("lists each action once in the app menu", () => {
    for (const mac of [true, false]) {
      const actions = appMenu(mac).flatMap(({ items }) => items.filter((item) => item !== "separator"));
      expect(new Set(actions).size).toBe(actions.length);
    }
  });
});
