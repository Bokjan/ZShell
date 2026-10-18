/**
 * Where among the tabs of a tab strip a tab dropped at `x` (CSS pixels from the left of the
 * page) goes: past each tab whose middle is left of it.
 */
export function tabIndexAt(strip: HTMLElement, x: number): number {
  return [...strip.querySelectorAll<HTMLElement>(".tab")].filter((tab) => {
    const rect = tab.getBoundingClientRect();
    return rect.left + rect.width / 2 < x;
  }).length;
}

/**
 * Where a tab put at `index` among the tabs of a strip goes, as a drop marker shows it: the
 * left edge of the tab there, or the right edge of the last one, in CSS pixels from the start
 * of the strip's content.
 */
export function tabEdgeAt(strip: HTMLElement, index: number): number {
  const tabs = strip.querySelectorAll<HTMLElement>(".tab");
  if (index < tabs.length) return tabs[index].offsetLeft;
  const last = tabs[tabs.length - 1];
  return last ? last.offsetLeft + last.offsetWidth : 0;
}

/** The window's tab strip (there is one). */
export const tabStrip = () => document.querySelector<HTMLElement>(".tab-strip");

/** A tab of the strip and where it is laid out, in CSS pixels from the start of the strip's content. */
export interface TabSlot {
  key: number;
  left: number;
  width: number;
}

/**
 * Where a tab dragged along the strip goes, the pointer at `x` (from the start of the strip's
 * content), and the tab it rests on (see `useTabDrag`). `others` are laid out as if the
 * dragged tab (`size` wide) were gone, and it goes at `index` among them now: those before
 * it show where they are, the others `size` further on. The pointer over the middle half of
 * one of them rests on it and leaves the index as it is; elsewhere, the dragged tab goes past
 * each one whose middle is before the pointer, which then slides over to make room.
 */
export function dragSlot(others: TabSlot[], size: number, index: number, x: number): { index: number; resting: number | null } {
  const shown = (i: number) => others[i].left + (i >= index ? size : 0);
  for (let i = 0; i < others.length; i++) {
    const { width, key } = others[i];
    if (x >= shown(i) + width / 4 && x <= shown(i) + (width * 3) / 4) return { index, resting: key };
  }
  return { index: others.filter((tab, i) => shown(i) + tab.width / 2 < x).length, resting: null };
}

/** Where each tab of the strip is on screen now, by key, transforms included. */
export const tabLefts = (strip: HTMLElement) =>
  new Map([...strip.querySelectorAll<HTMLElement>(".tab")].map((tab) => [tab.dataset.key!, tab.getBoundingClientRect().left]));

/** Slides each tab of the strip from where `from` says it was on screen to where it is now. */
export function slideTabs(strip: HTMLElement, from: Map<string, number>, duration: number) {
  for (const tab of strip.querySelectorAll<HTMLElement>(".tab")) {
    const left = from.get(tab.dataset.key!);
    const dx = left === undefined ? 0 : left - tab.getBoundingClientRect().left;
    if (Math.abs(dx) >= 1) tab.animate([{ transform: `translateX(${dx}px)` }, { transform: "none" }], { duration, easing: "ease-out" });
  }
}
