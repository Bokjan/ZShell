/**
 * Whether a key opens a menu from the keyboard: the menu key, or Shift+F10. React's events and
 * the DOM's alike (the terminal handles its keys itself).
 */
export const isMenuKey = (e: { key: string; shiftKey: boolean }) => e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey);

/** When a menu was last opened with a menu key. */
let openedAt = -Infinity;

/** Records that a menu key (see `isMenuKey`) has just opened a menu. */
export function openedByMenuKey() {
  openedAt = performance.now();
}

/**
 * Whether a `contextmenu` event follows a menu key that has just opened a menu: Windows sends
 * one for the key as well, to the element with the focus (a list, rather than its highlighted
 * row), which would replace that menu with the one for where the pointer is. Such an event is
 * cancelled, and the caller leaves the menu as it is.
 */
export function followsMenuKey(e: { preventDefault(): void; stopPropagation(): void }): boolean {
  if (performance.now() - openedAt > 500) return false;
  e.preventDefault();
  e.stopPropagation();
  return true;
}
