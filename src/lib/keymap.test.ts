import { describe, expect, it } from "vitest";

import { actionOf, labelsOn, matches, paneSideOf, shortcutRows, tabIndexOf } from "./keymap";

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
});
