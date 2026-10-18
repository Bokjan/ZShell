/**
 * Where among the tabs of a tab strip a tab dropped at `x` (CSS pixels from the left of the
 * page) goes: past each tab whose middle is left of it. `except`: the tab being dragged,
 * which doesn't count.
 */
export function tabIndexAt(strip: HTMLElement, x: number, except?: number): number {
  return [...strip.querySelectorAll<HTMLElement>(".tab")].filter((tab) => {
    if (except !== undefined && tab.dataset.key === String(except)) return false;
    const rect = tab.getBoundingClientRect();
    return rect.left + rect.width / 2 < x;
  }).length;
}

/** The window's tab strip (there is one). */
export const tabStrip = () => document.querySelector<HTMLElement>(".tab-strip");

/**
 * The tab of the strip (other than `except`) whose middle half `x` is over: a tab dragged
 * there rests on it rather than moving past it, and shows it after a moment (see `useTabDrag`).
 */
export function tabRestingAt(strip: HTMLElement, x: number, except?: number): number | null {
  for (const tab of strip.querySelectorAll<HTMLElement>(".tab")) {
    if (except !== undefined && tab.dataset.key === String(except)) continue;
    const rect = tab.getBoundingClientRect();
    if (x >= rect.left + rect.width / 4 && x <= rect.right - rect.width / 4) return Number(tab.dataset.key);
  }
  return null;
}
