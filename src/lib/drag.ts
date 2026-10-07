import type { MouseEvent as ReactMouseEvent } from "react";

/**
 * Follows a splitter drag until the mouse is released, reporting the pointer's x position.
 * While dragging, `body.resizing` keeps the resize cursor and stops terminals from
 * swallowing the mouse.
 */
export function dragHorizontally(e: ReactMouseEvent, onMove: (clientX: number) => void, onEnd?: () => void) {
  e.preventDefault();
  const move = (ev: MouseEvent) => onMove(ev.clientX);
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    document.body.classList.remove("resizing");
    onEnd?.();
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  document.body.classList.add("resizing");
}
