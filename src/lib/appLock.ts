/**
 * Whether the app lock covers the window (set by `AppLockGate`), for what acts outside React
 * or on its own: shortcuts, the dialog stack's keys, announcements, files dropped on the
 * window, and native dialogs a session opens by itself (ZMODEM), which would show over the
 * lock screen.
 */
let locked = false;
const waiting = new Set<() => void>();

export const isAppLocked = () => locked;

export function setAppLocked(value: boolean) {
  locked = value;
  if (locked || waiting.size === 0) return;
  const callbacks = [...waiting];
  waiting.clear();
  for (const callback of callbacks) callback();
}

/** Calls `callback` now, or once the app is unlocked; returns a function that cancels it. */
export function whenUnlocked(callback: () => void): () => void {
  if (!locked) {
    callback();
    return () => {};
  }
  waiting.add(callback);
  return () => void waiting.delete(callback);
}
