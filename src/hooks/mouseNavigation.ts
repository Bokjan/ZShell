import { useEffect, useEffectEvent, type RefObject } from "react";

import { useWindowEvent } from "./windowEvents";

/** `MouseEvent.button` of the mouse's back and forward buttons (the side buttons). */
const BACK = 3;
const FORWARD = 4;

/**
 * Calls `handler` as the mouse's back or forward button is pressed over `ref`'s element, with
 * whether it was forward. WebView2 (Windows) hands the page those buttons, and goes back in the
 * page's history as one is released unless that is prevented; WebKit gives the page none on
 * Linux, and on macOS gives it a middle click in versions before 2026, so there the backend
 * takes them from the web view and sends where they were pressed (`mouse-navigation`).
 */
export function useMouseNavigation(ref: RefObject<HTMLElement | null>, handler: (forward: boolean) => void) {
  const onNavigate = useEffectEvent(handler);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const onMouseUp = (e: MouseEvent) => {
      if (e.button !== BACK && e.button !== FORWARD) return;
      e.preventDefault();
      onNavigate(e.button === FORWARD);
    };
    element.addEventListener("mouseup", onMouseUp);
    return () => element.removeEventListener("mouseup", onMouseUp);
  }, [ref]);

  useWindowEvent("mouse-navigation", ({ forward, x, y }) => {
    const target = document.elementFromPoint(x, y);
    if (target && ref.current?.contains(target)) onNavigate(forward);
  });
}
