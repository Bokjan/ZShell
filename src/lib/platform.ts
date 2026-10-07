export const isMac = navigator.userAgent.includes("Mac");

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
