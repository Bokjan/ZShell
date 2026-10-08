export const isMac = navigator.userAgent.includes("Mac");
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
