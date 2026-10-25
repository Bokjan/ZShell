import { useCallback, useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";

/**
 * Follows a press of the left mouse button until it is released, also outside the window,
 * where the page doesn't hear it (seen on the next move). Returns a function that stops
 * following without `onUp`.
 */
function followPress(onMove: (e: MouseEvent) => void, onUp: () => void): () => void {
  const move = (ev: MouseEvent) => {
    if ((ev.buttons & 1) === 0) return up();
    onMove(ev);
  };
  const stop = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
  };
  const up = () => {
    stop();
    onUp();
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  return stop;
}

/**
 * Follows a splitter drag until the mouse is released, reporting the pointer's x position
 * (or y, for a splitter between stacked areas). While dragging, `body.resizing` keeps the
 * resize cursor and stops terminals from swallowing the mouse.
 */
export function dragSplitter(e: ReactMouseEvent, axis: "x" | "y", onMove: (position: number) => void, onEnd?: () => void) {
  e.preventDefault();
  const classes = axis === "x" ? ["resizing"] : ["resizing", "rows"];
  document.body.classList.add(...classes);
  followPress(
    (ev) => onMove(axis === "x" ? ev.clientX : ev.clientY),
    () => {
      document.body.classList.remove(...classes);
      onEnd?.();
    },
  );
}

export const dragHorizontally = (e: ReactMouseEvent, onMove: (clientX: number) => void, onEnd?: () => void) =>
  dragSplitter(e, "x", onMove, onEnd);

/** A mouse press that becomes a drag once the pointer has moved far enough (see `usePressDrag`). */
export interface PressDrag {
  /** How far the pointer must move, in pixels: in any direction, or only along `x`. */
  threshold: number;
  axis?: "x";
  /** On `body` while dragging. */
  bodyClass?: string;
  /** Each move once it is a drag. */
  onMove(e: MouseEvent): void;
  /** The button was released (also outside the window); `moved` is whether it became a drag. */
  onEnd(moved: boolean): void;
}

/**
 * Returns a function that follows a mouse press (`PressDrag`) until the button is released.
 * One that is still going when the component unmounts stops, its listeners removed, without
 * `onEnd`.
 */
export function usePressDrag(): (e: ReactMouseEvent, drag: PressDrag) => void {
  const stopCurrent = useRef<(() => void) | null>(null);
  useEffect(() => () => stopCurrent.current?.(), []);
  return useCallback((e: ReactMouseEvent, drag: PressDrag) => {
    const [startX, startY] = [e.clientX, e.clientY];
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved) {
        const distance = drag.axis === "x" ? Math.abs(ev.clientX - startX) : Math.hypot(ev.clientX - startX, ev.clientY - startY);
        if (distance < drag.threshold) return;
        moved = true;
        if (drag.bodyClass) document.body.classList.add(drag.bodyClass);
      }
      drag.onMove(ev);
    };
    const end = () => {
      if (drag.bodyClass) document.body.classList.remove(drag.bodyClass);
      stopCurrent.current = null;
    };
    stopCurrent.current?.();
    const unfollow = followPress(move, () => {
      end();
      drag.onEnd(moved);
    });
    stopCurrent.current = () => {
      unfollow();
      end();
    };
  }, []);
}
