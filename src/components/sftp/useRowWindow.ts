import { useEffect, useState, type RefObject } from "react";

import { rowSegments, type Segment } from "../../lib/rowWindow";

/** Lists shorter than this draw all their rows. */
const WINDOW_FROM = 500;
/** Rows drawn beyond those in view, on each side. */
const OVERSCAN = 30;
/** A row's height until one has been measured. */
const GUESS = 24;

/**
 * Which rows of a list of `count` to draw: with tens of thousands of files in a folder, all of
 * them would take seconds to show and much memory, so a long list draws those in view of
 * `listRef` (its scrolling element) and the rows to `keep`, leaving the others' height empty.
 * `null` for a short list, which draws them all. The rows' height is measured as the list
 * scrolls or resizes (it changes with the text size).
 */
export function useRowWindow(listRef: RefObject<HTMLElement | null>, count: number, keep: number[]) {
  const long = count >= WINDOW_FROM;
  const [view, setView] = useState({ top: 0, height: 0, rowHeight: GUESS });

  useEffect(() => {
    const list = listRef.current;
    if (!long || !list) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const row = list.querySelector("tbody tr[data-path]");
      const measured = row?.getBoundingClientRect().height ?? 0;
      setView((view) => {
        const next = { top: list.scrollTop, height: list.clientHeight, rowHeight: measured > 0 ? measured : view.rowHeight };
        return next.top === view.top && next.height === view.height && next.rowHeight === view.rowHeight ? view : next;
      });
    };
    const schedule = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    list.addEventListener("scroll", schedule, { passive: true });
    const observer = new ResizeObserver(schedule);
    observer.observe(list);
    return () => {
      cancelAnimationFrame(frame);
      list.removeEventListener("scroll", schedule);
      observer.disconnect();
    };
  }, [listRef, long]);

  if (!long) return null;
  const { top, height, rowHeight } = view;
  const start = Math.floor(top / rowHeight) - OVERSCAN;
  const end = Math.ceil((top + height) / rowHeight) + OVERSCAN;
  return { segments: rowSegments(count, start, end, keep), rowHeight };
}

export type { Segment };
