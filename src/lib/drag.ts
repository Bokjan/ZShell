import type { MouseEvent as ReactMouseEvent } from "react";

/**
 * Follows a splitter drag until the mouse is released, reporting the pointer's x position
 * (or y, for a splitter between stacked areas). While dragging, `body.resizing` keeps the
 * resize cursor and stops terminals from swallowing the mouse.
 */
export function dragSplitter(
  e: ReactMouseEvent,
  axis: "x" | "y",
  onMove: (position: number) => void,
  onEnd?: () => void,
) {
  e.preventDefault();
  const move = (ev: MouseEvent) => onMove(axis === "x" ? ev.clientX : ev.clientY);
  const classes = axis === "x" ? ["resizing"] : ["resizing", "rows"];
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    document.body.classList.remove(...classes);
    onEnd?.();
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  document.body.classList.add(...classes);
}

export const dragHorizontally = (e: ReactMouseEvent, onMove: (clientX: number) => void, onEnd?: () => void) =>
  dragSplitter(e, "x", onMove, onEnd);
