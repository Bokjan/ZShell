import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** Space kept between a tooltip and its element or the window's edges. */
const MARGIN = 6;
/** How long the pointer rests on an element before its tooltip shows. */
const DELAY = 150;

/**
 * A tooltip for `anchor`: below it, or above when there is no room, and within the window. It
 * goes on the body, so that a dialog's scrolling area doesn't clip it.
 */
export function TooltipPopup({ anchor, text }: { anchor: Element; text: string }) {
  const popup = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (!popup.current) return;
    const target = anchor.getBoundingClientRect();
    const { width, height } = popup.current.getBoundingClientRect();
    const left = Math.max(MARGIN, Math.min(target.left + target.width / 2 - width / 2, window.innerWidth - width - MARGIN));
    const below = target.bottom + MARGIN;
    const top = below + height + MARGIN <= window.innerHeight ? below : Math.max(MARGIN, target.top - height - MARGIN);
    setPosition({ left, top });
  }, [anchor, text]);

  return createPortal(
    <div ref={popup} className="tooltip" role="tooltip" style={position ?? { visibility: "hidden" }}>
      {text}
    </div>,
    document.body,
  );
}

/**
 * Shows the `title` of the element under the pointer in a tooltip drawn by the page, for the
 * whole app: WKWebView shows no `title` tooltips at all. While the pointer is on the element,
 * its `title` is taken off so that WebView2 doesn't show its own tooltip as well, and put back
 * when the pointer leaves (unless React has set a new one meanwhile).
 */
export function Tooltips() {
  const [shown, setShown] = useState<{ anchor: Element; text: string } | null>(null);

  useEffect(() => {
    let current: Element | null = null;
    let saved = "";
    let timer: number | undefined;

    const hide = () => {
      window.clearTimeout(timer);
      if (current && !current.hasAttribute("title")) current.setAttribute("title", saved);
      current = null;
      setShown(null);
    };
    const onOver = (e: MouseEvent) => {
      const anchor = e.target instanceof Element ? e.target.closest("[title]") : null;
      if (anchor === current) return;
      hide();
      const text = anchor?.getAttribute("title");
      if (!anchor || !text) return;
      current = anchor;
      saved = text;
      anchor.removeAttribute("title");
      timer = window.setTimeout(() => setShown({ anchor, text }), DELAY);
    };
    // Leaving the window.
    const onOut = (e: MouseEvent) => {
      if (!e.relatedTarget) hide();
    };

    document.addEventListener("mouseover", onOver, true);
    document.addEventListener("mouseout", onOut, true);
    // Clicking, typing or scrolling dismisses it, as native tooltips are.
    for (const type of ["mousedown", "keydown", "wheel"]) document.addEventListener(type, hide, true);
    window.addEventListener("blur", hide);
    return () => {
      hide();
      document.removeEventListener("mouseover", onOver, true);
      document.removeEventListener("mouseout", onOut, true);
      for (const type of ["mousedown", "keydown", "wheel"]) document.removeEventListener(type, hide, true);
      window.removeEventListener("blur", hide);
    };
  }, []);

  return shown && shown.anchor.isConnected ? <TooltipPopup anchor={shown.anchor} text={shown.text} /> : null;
}
