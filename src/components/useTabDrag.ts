import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type RefObject } from "react";

import type { Point } from "../ipc/bindings";
import type { DropTarget } from "../ipc/windows";
import type { Tab } from "../lib/panes";
import type { TabRest } from "../lib/tabRest";
import { tabIndexAt, tabRestingAt } from "../lib/tabStrip";
import { WindowDrag } from "../lib/windowDrag";

/** How far the pointer moves before a press on a tab becomes a drag. */
export const DRAG_THRESHOLD = 5;
/** While dragging a tab this close to an edge of the strip, the strip scrolls. */
const DRAG_SCROLL_EDGE = 24;
const DRAG_SCROLL_STEP = 12;

interface Options {
  /** The tabs as they are now, and the one shown. */
  tabsRef: RefObject<Tab[]>;
  activeKeyRef: RefObject<number | null>;
  /** Shows a tab. */
  onShow(key: number): void;
  /** The tab a dragged one rests on, shown after a moment. */
  tabRest: TabRest;
  stripRef: RefObject<HTMLDivElement | null>;
  /** Moves tab `key` to `index` among the others, where it was dropped. */
  onMove(key: number, index: number): void;
  /** Where among the other tabs the dragged tab `key` goes if dropped now, or nowhere (null). */
  onDropAt(key: number, index: number | null): void;
  canMoveToWindow(key: number): boolean;
  onDropOut(key: number, target: DropTarget, grab: Point): void;
  /**
   * The tab is dragged to (`x`, `y`) over the page, or no longer (null): whether it would go
   * next to a pane of the tab shown if dropped there (see `usePaneDrag`).
   */
  onOverPanes(key: number, at: Point | null): boolean;
  /** The tab is dropped where `onOverPanes` last said it would join the tab shown: it does. */
  onDropOnPanes(key: number): void;
}

/**
 * Dragging the tab bar's tabs: `startDrag` on a press, and the tab being dragged, which
 * `leaving` the window if dropped now. What a drag listens to goes with the tab bar.
 */
export function useTabDrag({
  tabsRef,
  activeKeyRef,
  onShow,
  tabRest,
  stripRef,
  onMove,
  onDropAt,
  canMoveToWindow,
  onDropOut,
  onOverPanes,
  onDropOnPanes,
}: Options) {
  const [dragging, setDragging] = useState<number | null>(null);
  // The dragged tab would leave the window if dropped now.
  const [leaving, setLeaving] = useState(false);
  /** Stops the drag going on, if any. */
  const stopRef = useRef<(() => void) | null>(null);
  useEffect(() => () => stopRef.current?.(), []);

  // The tab stays in its place while dragged: over the edge of another tab, a line shows where
  // it goes if dropped (`onDropAt`), as it does for a tab coming from another window; over the
  // middle half of another tab, it rests on it, which shows that tab after a moment, to drop
  // the dragged one on its panes, where it joins them. Dragged out of the window, the tab goes
  // where the backend says it is over when dropped (another window's tab bar, or a window of
  // its own). Dropped anywhere else, or after Escape, it stays where it was. Unless it joins
  // another tab or leaves the window, the tab shown before is shown again. Pressing doesn't
  // take focus from the terminal. Mouse events rather than HTML5 drag and drop, which Tauri's
  // file drop handling intercepts on Windows.
  const startDrag = (e: ReactMouseEvent, key: number) => {
    if (e.button !== 0) return;
    stopRef.current?.();
    e.preventDefault();
    const grab = { x: e.clientX, y: e.clientY };
    const startIndex = tabsRef.current.findIndex((tab) => tab.key === key);
    const startActive = activeKeyRef.current;
    let moved = false;
    // Whether it can go to another window, as the drag starts.
    let movable = false;
    // It would join the panes of the tab shown if dropped now.
    let overPanes = false;
    // Where among the other tabs it goes if dropped now, if anywhere.
    let dropIndex: number | null = null;
    const dropAt = (index: number | null) => {
      if (index === dropIndex) return;
      dropIndex = index;
      onDropAt(key, index);
    };
    let ended = false;
    // The tab bar went with the drag: nothing is left to drop the tab out of.
    let gone = false;
    // Where the pointer last was: outside the page or not.
    let outside = false;
    const isOutside = (ev: MouseEvent) =>
      ev.clientX < 0 || ev.clientY < 0 || ev.clientX >= window.innerWidth || ev.clientY >= window.innerHeight;
    const drag = new WindowDrag({
      target: (target) => {
        // A window's only tab has nowhere new to go.
        const leaves = target.kind === "window" || (target.kind !== "here" && tabsRef.current.length > 1);
        if (!ended) setLeaving(leaves);
      },
      native: () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        window.removeEventListener("keydown", keyDown, true);
      },
      // Dropped outside the page, or cancelled (Escape).
      nativeEnd: (dropped) => {
        dropAt(null);
        finish(dropped);
      },
    });
    const move = (ev: MouseEvent) => {
      outside = isOutside(ev);
      // Released where the page didn't hear it.
      if (ev.buttons === 0) {
        finish(true);
        return;
      }
      if (!moved) {
        // Any way: straight down out of the window too.
        if (Math.hypot(ev.clientX - grab.x, ev.clientY - grab.y) < DRAG_THRESHOLD) return;
        moved = true;
        movable = canMoveToWindow(key);
        setDragging(key);
      }
      if (movable) drag.move(outside);
      const strip = stripRef.current!;
      const bounds = strip.getBoundingClientRect();
      overPanes = !outside && ev.clientY > bounds.bottom && onOverPanes(key, { x: ev.clientX, y: ev.clientY });
      const inStrip = !outside && ev.clientY >= bounds.top && ev.clientY <= bounds.bottom;
      const over = inStrip ? tabRestingAt(strip, ev.clientX, key) : null;
      tabRest.rest(over);
      if (!inStrip || over !== null) return dropAt(null);
      if (ev.clientX < bounds.left + DRAG_SCROLL_EDGE) strip.scrollLeft -= DRAG_SCROLL_STEP;
      else if (ev.clientX > bounds.right - DRAG_SCROLL_EDGE) strip.scrollLeft += DRAG_SCROLL_STEP;
      // Where it is already: no line.
      const index = tabIndexAt(strip, ev.clientX, key);
      dropAt(index === startIndex ? null : index);
    };
    const keyDown = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      overPanes = false;
      dropAt(null);
      finish(false);
    };
    const finish = (dropped: boolean) => {
      if (ended) return;
      ended = true;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", keyDown, true);
      setDragging(null);
      setLeaving(false);
      tabRest.rest(null);
      const index = dropped && !gone ? dropIndex : null;
      dropAt(null);
      if (index !== null) onMove(key, index);
      // Joining takes the place `onOverPanes` last found, which clearing it would forget.
      const joins = dropped && overPanes && !outside && !gone;
      if (joins) onDropOnPanes(key);
      else onOverPanes(key, null);
      // A tab shown on the way, where nothing was dropped: the one shown before comes back.
      const showBack = () => {
        if (!joins && !gone && startActive !== null && activeKeyRef.current !== startActive) onShow(startActive);
      };
      if (!movable) return showBack();
      void drag.end(dropped && !joins, outside).then((target) => {
        if (target && target.kind !== "here" && !gone) onDropOut(key, target, grab);
        else showBack();
      }, console.error);
    };
    const up = (ev: MouseEvent) => {
      outside = isOutside(ev);
      if (outside) dropAt(null);
      finish(true);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", keyDown, true);
    // Gone with the tab bar mid-drag: its listeners go too, and the backend hears that the drag
    // ended, which takes down the drop marker another window may show.
    stopRef.current = () => {
      gone = true;
      finish(false);
    };
  };

  return { dragging, leaving, startDrag };
}
