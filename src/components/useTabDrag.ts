import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type RefObject } from "react";

import type { Point } from "../ipc/bindings";
import { listenHere } from "../ipc/events";
import { tabDrag, type DropTarget } from "../ipc/windows";
import type { Tab } from "../lib/panes";
import { tabIndexAt } from "../lib/tabStrip";

/** How far the pointer moves before a press on a tab becomes a drag. */
const DRAG_THRESHOLD = 5;
/** While dragging a tab this close to an edge of the strip, the strip scrolls. */
const DRAG_SCROLL_EDGE = 24;
const DRAG_SCROLL_STEP = 12;

interface Options {
  /** The tabs as they are now. */
  tabsRef: RefObject<Tab[]>;
  stripRef: RefObject<HTMLDivElement | null>;
  onMove(key: number, index: number): void;
  canMoveToWindow(key: number): boolean;
  onDropOut(key: number, target: DropTarget, grab: Point): void;
}

/**
 * Dragging the tab bar's tabs: `startDrag` on a press, and the tab being dragged, which
 * `leaving` the window if dropped now. What a drag listens to goes with the tab bar.
 */
export function useTabDrag({ tabsRef, stripRef, onMove, canMoveToWindow, onDropOut }: Options) {
  const [dragging, setDragging] = useState<number | null>(null);
  // The dragged tab would leave the window if dropped now.
  const [leaving, setLeaving] = useState(false);
  /** Stops the drag going on, if any. */
  const stopRef = useRef<(() => void) | null>(null);
  useEffect(() => () => stopRef.current?.(), []);

  // Reorders live while dragging: the tab moves past each neighbor whose middle the pointer
  // crosses. Pressing doesn't take focus from the terminal. Mouse events rather than HTML5
  // drag and drop, which Tauri's file drop handling intercepts on Windows. Dragged out of the
  // window, the tab goes where the backend says it is over when dropped (another window's tab
  // bar, or a window of its own); Escape puts it back.
  const startDrag = (e: ReactMouseEvent, key: number) => {
    if (e.button !== 0) return;
    stopRef.current?.();
    e.preventDefault();
    const grab = { x: e.clientX, y: e.clientY };
    // Names the drag for the backend, which ignores answers that come back after it ended.
    const id = crypto.randomUUID();
    const startIndex = tabsRef.current.findIndex((tab) => tab.key === key);
    let moved = false;
    // Whether it can go to another window, as the drag starts.
    let movable = false;
    // One question about where it would go at a time, and the latest position after it.
    let asking = false;
    let askAgain = false;
    let ended = false;
    // The tab bar went with the drag: nothing is left to drop the tab out of.
    let gone = false;
    // Where the pointer last was: outside the page or not.
    let outside = false;
    // The system's drag took over (Wayland): the page hears no more of the mouse, and waits
    // for the end of that drag.
    let native = false;
    let stopNative: Promise<() => void> | null = null;
    const isOutside = (ev: MouseEvent) =>
      ev.clientX < 0 || ev.clientY < 0 || ev.clientX >= window.innerWidth || ev.clientY >= window.innerHeight;
    const ask = () => {
      if (asking) {
        askAgain = true;
        return;
      }
      asking = true;
      void tabDrag
        .move(id, outside)
        .then((target) => {
          if (target.kind === "native") goNative();
          // A window's only tab has nowhere new to go.
          const leaves = target.kind === "window" || (target.kind !== "here" && tabsRef.current.length > 1);
          if (!ended) setLeaving(leaves);
        }, console.error)
        .finally(() => {
          asking = false;
          if (askAgain && !ended) {
            askAgain = false;
            ask();
          }
        });
    };
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
      if (movable) ask();
      if (outside) return;
      const strip = stripRef.current!;
      const bounds = strip.getBoundingClientRect();
      if (ev.clientX < bounds.left + DRAG_SCROLL_EDGE) strip.scrollLeft -= DRAG_SCROLL_STEP;
      else if (ev.clientX > bounds.right - DRAG_SCROLL_EDGE) strip.scrollLeft += DRAG_SCROLL_STEP;
      const index = tabIndexAt(strip, ev.clientX, key);
      if (tabsRef.current.findIndex((tab) => tab.key === key) !== index) onMove(key, index);
    };
    const keyDown = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      if (moved) onMove(key, startIndex);
      finish(false);
    };
    const goNative = () => {
      if (native || ended) return;
      native = true;
      const unlisten = listenHere("tab-drag-native-end", ({ payload }) => {
        if (payload.id !== id) return;
        void unlisten.then((f) => f());
        stopNative = null;
        // Cancelled (Escape): back where it was, as in the page.
        if (!payload.dropped && moved) onMove(key, startIndex);
        finish(payload.dropped);
      });
      stopNative = unlisten;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", keyDown, true);
    };
    const finish = (dropped: boolean) => {
      if (ended) return;
      ended = true;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      window.removeEventListener("keydown", keyDown, true);
      setDragging(null);
      setLeaving(false);
      if (!movable) return;
      void tabDrag.end(id, dropped, outside).then((target) => target && !gone && onDropOut(key, target, grab), console.error);
    };
    const up = (ev: MouseEvent) => {
      outside = isOutside(ev);
      finish(true);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    window.addEventListener("keydown", keyDown, true);
    // Gone with the tab bar mid-drag: its listeners go too, and the backend hears that the drag
    // ended, which takes down the drop marker another window may show.
    stopRef.current = () => {
      void stopNative?.then((f) => f());
      gone = true;
      finish(false);
    };
  };

  return { dragging, leaving, startDrag };
}
