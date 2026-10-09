export const isMac = navigator.userAgent.includes("Mac");

/**
 * Whether a key belongs to an input method's composition (Chinese, Japanese…): its Enter
 * confirms a candidate and its Escape cancels one, rather than submitting or closing. WebKit
 * ends the composition before the confirming Enter's keydown, which then only `keyCode` 229
 * tells apart.
 */
export function isComposing(e: KeyboardEvent | { nativeEvent: KeyboardEvent }): boolean {
  const event = "nativeEvent" in e ? e.nativeEvent : e;
  return event.isComposing || event.keyCode === 229;
}
/** Windows draws its own window buttons (see `WindowControls`). */
export const isWindows = navigator.userAgent.includes("Windows");

/** Whether the event has the app's shortcut modifiers: ⇧⌘ on macOS, Ctrl+Shift elsewhere. */
export const hasShiftShortcutModifiers = (e: KeyboardEvent) =>
  e.shiftKey && !e.altKey && (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey);

/** Platform-style label for a ⇧⌘ / Ctrl+Shift shortcut, e.g. "⇧⌘E" or "Ctrl+Shift+E". */
export const shiftShortcutLabel = (key: string) => (isMac ? `⇧⌘${key}` : `Ctrl+Shift+${key}`);

/** The find shortcut: ⌘F on macOS; Ctrl+Shift+F elsewhere, since Ctrl+F belongs to the shell. */
export const isFindShortcut = (e: KeyboardEvent) =>
  e.code === "KeyF" &&
  !e.altKey &&
  (isMac ? e.metaKey && !e.ctrlKey && !e.shiftKey : e.ctrlKey && e.shiftKey && !e.metaKey);

export const findShortcutLabel = isMac ? "⌘F" : "Ctrl+Shift+F";

/** The settings shortcut: ⌘, on macOS, Ctrl+, elsewhere. */
export const isSettingsShortcut = (e: KeyboardEvent) =>
  e.code === "Comma" && !e.altKey && !e.shiftKey && (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey);

export const settingsShortcutLabel = isMac ? "⌘," : "Ctrl+,";

/** Focuses the session search: ⌘K on macOS; Ctrl+Shift+K elsewhere, since Ctrl+K belongs to the shell. */
export const isSearchShortcut = (e: KeyboardEvent) =>
  e.code === "KeyK" &&
  !e.altKey &&
  (isMac ? e.metaKey && !e.ctrlKey && !e.shiftKey : e.ctrlKey && e.shiftKey && !e.metaKey);

export const searchShortcutLabel = isMac ? "⌘K" : "Ctrl+Shift+K";

/** Opens a local terminal tab: ⌘T on macOS; Ctrl+Shift+T elsewhere, since Ctrl+T belongs to the shell. */
export const isNewTabShortcut = (e: KeyboardEvent) =>
  e.code === "KeyT" &&
  !e.altKey &&
  (isMac ? e.metaKey && !e.ctrlKey && !e.shiftKey : e.ctrlKey && e.shiftKey && !e.metaKey);

export const newTabShortcutLabel = isMac ? "⌘T" : "Ctrl+Shift+T";

/**
 * Terminal clipboard keys outside macOS, where the native Edit menu handles ⌘C / ⌘V:
 * Ctrl+Shift+C / Ctrl+Insert copy; Ctrl+Shift+V, Ctrl+V and Shift+Insert paste. Plain Ctrl+C
 * copies only while there is a selection (the caller checks), as in Windows Terminal.
 */
export function clipboardKey(e: KeyboardEvent): "copy" | "copyIfSelected" | "paste" | null {
  if (isMac || e.altKey || e.metaKey) return null;
  if (e.code === "Insert") return e.ctrlKey && !e.shiftKey ? "copy" : e.shiftKey && !e.ctrlKey ? "paste" : null;
  if (!e.ctrlKey) return null;
  if (e.code === "KeyC") return e.shiftKey ? "copy" : "copyIfSelected";
  if (e.code === "KeyV") return "paste";
  return null;
}

export const copyShortcutLabel = isMac ? "⌘C" : "Ctrl+Shift+C";
export const pasteShortcutLabel = isMac ? "⌘V" : "Ctrl+Shift+V";
/** Every key that copies and pastes (see `clipboardKey`), for the list of shortcuts. */
export const allCopyShortcutsLabel = isMac ? "⌘C" : "Ctrl+Shift+C, Ctrl+Insert";
export const allPasteShortcutsLabel = isMac ? "⌘V" : "Ctrl+Shift+V, Ctrl+V, Shift+Insert";
/** Only macOS has a select-all shortcut in the terminal (Ctrl+A belongs to the shell). */
export const selectAllShortcutLabel = isMac ? "⌘A" : undefined;

/** Tab shortcuts: which tab to activate or close, if `e` is one. */
export type TabShortcut = { type: "next" } | { type: "previous" } | { type: "index"; index: number } | { type: "close" };

export function tabShortcut(e: KeyboardEvent): TabShortcut | null {
  // Ctrl+Tab on both platforms, as in browsers.
  if (e.code === "Tab" && e.ctrlKey && !e.altKey && !e.metaKey) return { type: e.shiftKey ? "previous" : "next" };
  // ⌘1–9 / Alt+1–9; 9 is the last tab.
  const digit = /^Digit([1-9])$/.exec(e.code);
  if (digit && !e.shiftKey && (isMac ? e.metaKey && !e.ctrlKey && !e.altKey : e.altKey && !e.ctrlKey && !e.metaKey)) {
    return { type: "index", index: Number(digit[1]) - 1 };
  }
  // ⌘W is a native menu item on macOS (see lib.rs).
  if (!isMac && e.code === "KeyW" && hasShiftShortcutModifiers(e)) return { type: "close" };
  return null;
}

export const closeTabShortcutLabel = isMac ? "⌘W" : "Ctrl+Shift+W";
export const nextTabShortcutLabel = "Ctrl+Tab";
export const previousTabShortcutLabel = "Ctrl+Shift+Tab";
export const goToTabShortcutLabel = isMac ? "⌘1 – ⌘8" : "Alt+1 – Alt+8";
export const lastTabShortcutLabel = isMac ? "⌘9" : "Alt+9";
/** ⇧⌘W closes the window on macOS (a native menu item); Windows has no shortcut for it. */
export const closeWindowShortcutLabel = isMac ? "⇧⌘W" : undefined;

/**
 * Splitting the focused pane: right of it (`row`) or below it (`column`). ⌘D / ⇧⌘D on macOS
 * as in iTerm2; Alt+Shift+= / Alt+Shift+- elsewhere as in Windows Terminal.
 */
export function splitShortcut(e: KeyboardEvent): "row" | "column" | null {
  if (isMac) {
    if (e.code !== "KeyD" || !e.metaKey || e.ctrlKey || e.altKey) return null;
    return e.shiftKey ? "column" : "row";
  }
  if (!e.altKey || !e.shiftKey || e.ctrlKey || e.metaKey) return null;
  if (e.code === "Equal" || e.code === "NumpadAdd") return "row";
  if (e.code === "Minus" || e.code === "NumpadSubtract") return "column";
  return null;
}

export const splitRightShortcutLabel = isMac ? "⌘D" : "Alt+Shift+=";
export const splitDownShortcutLabel = isMac ? "⇧⌘D" : "Alt+Shift+-";
export const paneFocusShortcutLabel = isMac ? "⌥⌘ ← ↑ → ↓" : "Ctrl+Alt+← ↑ → ↓";

const ARROWS: Record<string, "left" | "right" | "up" | "down"> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
};

/**
 * Moving the focus to the pane on a side: ⌥⌘ + arrow on macOS as in iTerm2; Ctrl+Alt + arrow
 * elsewhere, since shells read Alt + arrow (moving by words).
 */
export function paneFocusShortcut(e: KeyboardEvent): "left" | "right" | "up" | "down" | null {
  const side = ARROWS[e.code];
  if (!side || e.shiftKey || !e.altKey) return null;
  return (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey) ? side : null;
}
