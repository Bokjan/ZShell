import type { KeyboardEvent } from "react";

import { isComposing } from "./platform";

/** The highlighted item of a list of `count` items: `index` kept within the list, or -1 when it is empty. */
export const clampIndex = (index: number, count: number) => (count === 0 ? -1 : Math.min(Math.max(index, 0), count - 1));

/**
 * Keys for a list navigated from a text field that keeps the focus (a combobox, which points
 * screen readers at the highlighted item with `aria-activedescendant`): ↑ / ↓ move the
 * highlight from `index` (see `clampIndex`) within the list, and Enter picks the highlighted
 * item, if there is one and `pick` is given. Returns whether the key was handled, in which
 * case its default is prevented.
 */
export function navigateList(
  e: KeyboardEvent,
  count: number,
  index: number,
  move: (index: number) => void,
  pick?: (index: number) => void,
): boolean {
  if (isComposing(e)) return false;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    if (count > 0) move(clampIndex(e.key === "ArrowDown" ? index + 1 : index - 1, count));
  } else if (e.key === "Enter" && pick && index >= 0 && index < count) {
    pick(index);
  } else {
    return false;
  }
  e.preventDefault();
  return true;
}

/**
 * For a row of options that is one stop for Tab (radio buttons, tabs): the option that `key`
 * moves to from option `at` of `count`, or null for other keys. The arrow keys wrap around
 * (↑ / ↓ too unless `horizontalOnly`), Home and End go to the ends.
 */
export function rovingTarget(key: string, at: number, count: number, horizontalOnly = false): number | null {
  const last = count - 1;
  if (key === "ArrowRight" || (key === "ArrowDown" && !horizontalOnly)) return at >= last ? 0 : at + 1;
  if (key === "ArrowLeft" || (key === "ArrowUp" && !horizontalOnly)) return at <= 0 ? last : at - 1;
  if (key === "Home") return 0;
  if (key === "End") return last;
  return null;
}
