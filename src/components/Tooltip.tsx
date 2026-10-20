import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

/** Space kept between a tooltip and its element or the window's edges. */
const MARGIN = 6;
/** How long the pointer rests on an element before its tooltip shows. */
export const TOOLTIP_DELAY = 200;

/**
 * A tooltip for `anchor` (an element, or any area of the window): below it, or above when there
 * is no room, and within the window. It goes on the body, so that a dialog's scrolling area
 * doesn't clip it.
 */
export function TooltipPopup({ anchor, text }: { anchor: Pick<Element, "getBoundingClientRect">; text: string }) {
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
 * its `title` is kept off so that WebView2 doesn't show its own tooltip as well: taken off when
 * the pointer comes, and again if React sets a new one meanwhile (whose text is then shown),
 * and put back when the pointer leaves.
 */
export function Tooltips() {
  const [shown, setShown] = useState<{ anchor: Element; text: string } | null>(null);

  useEffect(() => {
    let current: Element | null = null;
    let saved = "";
    let timer: number | undefined;
    const retitled = new MutationObserver(() => {
      if (!current?.hasAttribute("title")) return;
      saved = current.getAttribute("title") ?? "";
      current.removeAttribute("title");
      setShown((tip) => (tip && saved ? { anchor: tip.anchor, text: saved } : null));
    });

    // On every key typed, too: nothing to do unless the pointer is on an element with a title.
    const hide = () => {
      if (!current) return;
      window.clearTimeout(timer);
      // Before its title is put back, which would count as React's.
      retitled.disconnect();
      if (!current.hasAttribute("title")) current.setAttribute("title", saved);
      current = null;
      setShown(null);
    };
    const onOver = (e: MouseEvent) => {
      const target = e.target instanceof Element ? e.target : null;
      const titled = target?.closest("[title]") ?? null;
      // The element shown has no title now, so `closest` passes over it: it stays while the
      // pointer moves within it (between its icon and its text), unless onto an element with a
      // title of its own inside it. Otherwise its title would come back under the pointer, and
      // WebView2's tooltip with it.
      const within = current && target && current.contains(target) && !(titled && current.contains(titled));
      const anchor = within ? current : titled;
      if (anchor === current) return;
      hide();
      const text = anchor?.getAttribute("title");
      if (!anchor || !text) return;
      current = anchor;
      saved = text;
      anchor.removeAttribute("title");
      retitled.observe(anchor, { attributes: true, attributeFilter: ["title"] });
      timer = window.setTimeout(() => saved && setShown({ anchor, text: saved }), TOOLTIP_DELAY);
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
