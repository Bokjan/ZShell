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
