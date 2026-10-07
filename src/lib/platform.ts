export const isMac = navigator.userAgent.includes("Mac");

/** Whether the event has the app's shortcut modifiers: ⇧⌘ on macOS, Ctrl+Shift elsewhere. */
export const hasShiftShortcutModifiers = (e: KeyboardEvent) =>
  e.shiftKey && !e.altKey && (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey);

/** Platform-style label for a ⇧⌘ / Ctrl+Shift shortcut, e.g. "⇧⌘E" or "Ctrl+Shift+E". */
export const shiftShortcutLabel = (key: string) => (isMac ? `⇧⌘${key}` : `Ctrl+Shift+${key}`);
