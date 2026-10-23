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
 * whole app: WKWebView shows no `title` tooltips at all. The web view must never show one of
 * its own beside it (WebView2 does, and so do some versions of WKWebView), which it does for
 * the nearest element with a `title` around the pointer: while the pointer is on an element,
 * its `title` and those of the elements around it are kept off. They are taken off when the
 * pointer comes, again if React sets a new one meanwhile (the element's own is then shown),
 * and put back when the pointer leaves.
 */
export function Tooltips() {
  const [shown, setShown] = useState<{ anchor: Element; text: string } | null>(null);

  useEffect(() => {
    let current: Element | null = null;
    /** The titles taken off: the element's own first, then those around it. */
    let taken: { element: Element; title: string }[] = [];
    let timer: number | undefined;
    const retitled = new MutationObserver((records) => {
      for (const { target } of records) {
        const entry = taken.find(({ element }) => element === target);
        if (!entry || !(target instanceof Element) || !target.hasAttribute("title")) continue;
        entry.title = target.getAttribute("title") ?? "";
        target.removeAttribute("title");
        if (target === current) setShown((tip) => (tip && entry.title ? { anchor: tip.anchor, text: entry.title } : null));
      }
    });

    // On every key typed, too: nothing to do unless the pointer is on an element with a title.
    const hide = () => {
      if (!current) return;
      window.clearTimeout(timer);
      // Before the titles are put back, which would count as React's.
      retitled.disconnect();
      for (const { element, title } of taken) {
        if (!element.hasAttribute("title")) element.setAttribute("title", title);
      }
      taken = [];
      current = null;
      setShown(null);
    };
    const onOver = (e: MouseEvent) => {
      const target = e.target instanceof Element ? e.target : null;
      // The element shown has no title now, so `closest` passes over it: it stays while the
      // pointer moves within it (between its icon and its text), unless onto an element with a
      // title of its own inside it. Otherwise its title would come back under the pointer, and
      // the web view's tooltip with it.
      const inner = target?.closest("[title]") ?? null;
      if (current && target && current.contains(target) && !(inner && current.contains(inner))) return;
      hide();
      // Found again with the titles around the element that was shown put back.
      const anchor = target?.closest("[title]") ?? null;
      const text = anchor?.getAttribute("title");
      if (!anchor || !text) return;
      current = anchor;
      for (let element: Element | null = anchor; element; element = element.parentElement?.closest("[title]") ?? null) {
        taken.push({ element, title: element.getAttribute("title") ?? "" });
        element.removeAttribute("title");
        retitled.observe(element, { attributes: true, attributeFilter: ["title"] });
      }
      timer = window.setTimeout(() => {
        const title = taken[0]?.title;
        if (title) setShown({ anchor, text: title });
      }, TOOLTIP_DELAY);
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
