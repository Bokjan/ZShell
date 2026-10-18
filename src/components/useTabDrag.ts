import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type RefObject } from "react";

import type { Point } from "../ipc/bindings";
import type { DropTarget } from "../ipc/windows";
import type { Tab } from "../lib/panes";
import type { TabRest } from "../lib/tabRest";
import { dragSlot, slideTabs, tabLefts, type TabSlot } from "../lib/tabStrip";
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
  /**
   * Moves tab `key` to `index` among the others, where it was dropped; the tabs were `from`
   * on screen (see `tabLefts`), from where they slide to their places.
   */
  onMove(key: number, index: number, from: Map<string, number>): void;
  /** How long a tab dropped where it was takes to slide back into its place; 0 for no motion. */
  slideMs: number;
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
  slideMs,
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

  // As in browsers, the dragged tab follows the pointer along the strip, and each tab it goes
  // past slides over to make room (`dragSlot`); dropped, it slides into its new place. Only the
  // tabs' transforms change meanwhile: the order changes once, when it is dropped. Over the
  // middle half of another tab it rests on it, which shows that tab after a moment, to drop
  // the dragged one on its panes, where it joins them. Off the strip it goes back to its place:
  // dragged out of the window, it goes where the backend says it is over when dropped (another
  // window's tab bar, or a window of its own). Dropped anywhere else, or after Escape, it stays
  // where it was. Unless it joins another tab or leaves the window, the tab shown before is
  // shown again. Pressing doesn't take focus from the terminal. Mouse events rather than HTML5
  // drag and drop, which Tauri's file drop handling intercepts on Windows.
  const startDrag = (e: ReactMouseEvent, key: number) => {
    if (e.button !== 0) return;
    stopRef.current?.();
    e.preventDefault();
    const grab = { x: e.clientX, y: e.clientY };
    const strip = stripRef.current!;
    const startIndex = tabsRef.current.findIndex((tab) => tab.key === key);
    const startActive = activeKeyRef.current;
    let moved = false;
    // Whether it can go to another window, as the drag starts.
    let movable = false;
    // It would join the panes of the tab shown if dropped now.
    let overPanes = false;
    // The pointer is over the strip, where the tab follows it.
    let inStrip = false;
    // Where it goes among the other tabs if dropped in the strip now.
    let index = startIndex;
    let ended = false;
    // The tab bar went with the drag: nothing is left to drop the tab out of.
    let gone = false;
    // Where the pointer last was: outside the page or not.
    let outside = false;
    /**
     * The strip as the drag starts, in CSS pixels from the start of its content: the dragged
     * tab, the space it takes (with what is between tabs), and the others laid out without it.
     */
    let layout: {
      element: HTMLElement;
      left: number;
      size: number;
      end: number;
      others: (TabSlot & { element: HTMLElement })[];
      count: number;
    } | null = null;
    // Where the pointer held the tab, from its left edge.
    let grabOffset = 0;
    const contentX = (clientX: number) => clientX - strip.getBoundingClientRect().left + strip.scrollLeft;
    const measure = () => {
      const elements = [...strip.querySelectorAll<HTMLElement>(".tab")];
      const at = elements.findIndex((tab) => tab.dataset.key === String(key));
      if (at < 0) return null;
      const element = elements[at];
      const last = elements[elements.length - 1];
      const size = at + 1 < elements.length ? elements[at + 1].offsetLeft - element.offsetLeft : element.offsetWidth;
      const others = elements
        .filter((_, i) => i !== at)
        .map((tab, i) => ({
          key: Number(tab.dataset.key),
          element: tab,
          left: tab.offsetLeft - (i >= at ? size : 0),
          width: tab.offsetWidth,
        }));
      return { element, left: element.offsetLeft, size, end: last.offsetLeft + last.offsetWidth, others, count: elements.length };
    };
    /** Shows the dragged tab with its left edge at `x`, the others making room for it; null: all in their places. */
    const show = (x: number | null) => {
      if (!layout) return;
      const { element, left, size, others } = layout;
      const target = x === null ? startIndex : index;
      others.forEach((tab, i) => {
        const shift = (i >= target ? size : 0) - (i >= startIndex ? size : 0);
        tab.element.style.transform = shift === 0 ? "" : `translateX(${shift}px)`;
      });
      element.classList.toggle("following", x !== null);
      element.style.transform = x === null ? "" : `translateX(${x - left}px)`;
    };

    const drag = new WindowDrag({
      target: (target) => {
        // A window's only tab has nowhere new to go.
        const leaves = target.kind === "window" || (target.kind !== "here" && tabsRef.current.length > 1);
        if (!ended) setLeaving(leaves);
      },
      native: () => {
        inStrip = false;
        tabRest.rest(null);
        show(null);
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        window.removeEventListener("keydown", keyDown, true);
      },
      // Dropped outside the page, or cancelled (Escape).
      nativeEnd: (dropped) => finish(dropped),
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
        layout = measure();
        if (!layout) return finish(false);
        grabOffset = contentX(grab.x) - layout.left;
        strip.classList.add("reordering");
        setDragging(key);
      }
      // A tab opened or closed meanwhile: the strip measured is no more.
      if (!layout || tabsRef.current.length !== layout.count) return finish(false);
      if (movable) drag.move(outside);
      const bounds = strip.getBoundingClientRect();
      overPanes = !outside && ev.clientY > bounds.bottom && onOverPanes(key, { x: ev.clientX, y: ev.clientY });
      inStrip = !outside && ev.clientY >= bounds.top && ev.clientY <= bounds.bottom;
      if (!inStrip) {
        tabRest.rest(null);
        return show(null);
      }
      if (ev.clientX < bounds.left + DRAG_SCROLL_EDGE) strip.scrollLeft -= DRAG_SCROLL_STEP;
      else if (ev.clientX > bounds.right - DRAG_SCROLL_EDGE) strip.scrollLeft += DRAG_SCROLL_STEP;
      const x = contentX(ev.clientX);
      const slot = dragSlot(layout.others, layout.size, index, x);
      index = slot.index;
      tabRest.rest(slot.resting);
      show(Math.max(0, Math.min(x - grabOffset, layout.end - layout.size)));
    };
    const keyDown = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      overPanes = false;
      inStrip = false;
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
      // From where it is shown now into its place: a new one if dropped in the strip.
      if (layout && !gone) {
        const from = tabLefts(strip);
        strip.classList.remove("reordering");
        show(null);
        if (dropped && inStrip && index !== startIndex) onMove(key, index, from);
        else if (slideMs > 0) slideTabs(strip, from, slideMs);
      }
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
      if (outside) inStrip = false;
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

const isOutside = (ev: MouseEvent) =>
  ev.clientX < 0 || ev.clientY < 0 || ev.clientX >= window.innerWidth || ev.clientY >= window.innerHeight;
