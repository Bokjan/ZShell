import { createContext, useContext, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";

import { isComposing } from "./platform";

/**
 * The open dialogs, as a stack: only the top one answers Escape and clicks on its backdrop,
 * so a dialog opened from another (a proxy from the settings) closes alone, and Tab moves
 * the focus only among its controls. Dialogs are ordered by when they first rendered, which
 * puts one opened from another above it even when both appear at once.
 */
interface Entry {
  hidden: boolean;
  escape(e: KeyboardEvent): void;
  element: RefObject<HTMLDivElement | null>;
}

const entries = new Map<number, Entry>();
let nextOrder = 0;
let listening = false;

/** The order of the top dialog shown, if any. */
function topOrder(): number | null {
  let top: number | null = null;
  for (const [order, entry] of entries) if (!entry.hidden && (top === null || order > top)) top = order;
  return top;
}

/** Whether a dialog (or the command palette) is shown over the tabs. */
export const isDialogOpen = () => topOrder() !== null;

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** The controls in `container` that Tab reaches, in order: enabled, shown and not hidden from it. */
export function focusables(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) =>
      el.tabIndex >= 0 &&
      !el.closest("[inert], [aria-hidden='true']") &&
      (el.checkVisibility ? el.checkVisibility({ visibilityProperty: true }) : el.getClientRects().length > 0),
  );
}

/** Keeps Tab and Shift+Tab within the top dialog, from its last control back to the first. */
function trapTab(e: KeyboardEvent, container: HTMLElement) {
  const items = focusables(container);
  e.preventDefault();
  if (items.length === 0) return;
  const at = items.indexOf(document.activeElement as HTMLElement);
  // From outside the dialog (the focus was lost to the page), to its first or last control.
  const next = at < 0 ? (e.shiftKey ? items.length - 1 : 0) : (at + (e.shiftKey ? -1 : 1) + items.length) % items.length;
  items[next].focus();
}

// Bubble phase, after the focused element: a control that handles Escape itself (a menu, a
// field that cancels an edit) prevents the default to keep the dialog open.
function onKeyDown(e: KeyboardEvent) {
  if (e.defaultPrevented || isComposing(e) || (e.key !== "Escape" && e.key !== "Tab")) return;
  const top = topOrder();
  if (top === null) return;
  const entry = entries.get(top)!;
  if (e.key === "Tab") {
    const container = entry.element.current;
    if (container && !e.altKey && !e.ctrlKey && !e.metaKey) trapTab(e, container);
    return;
  }
  e.preventDefault();
  entry.escape(e);
}

/**
 * Dialogs rendered inside a hidden part of the window (an inactive tab, a closed side panel)
 * stay hidden, and out of the stack, until it is shown again.
 */
export const DialogsHidden = createContext(false);

export interface DialogHandle {
  hidden: boolean;
  /** Whether this is the top dialog shown, which gets Escape and clicks on its backdrop. */
  isTop(): boolean;
  close(): void;
  /** The dialog's element, set by `Modal`. */
  element: RefObject<HTMLDivElement | null>;
}

interface Options {
  /** Instead of closing on Escape, e.g. to clear a search field first. */
  onEscape?(e: KeyboardEvent): void;
}

/** Registers a dialog while it is mounted; render it with `Modal`. */
export function useDialog(onClose: () => void, options: Options = {}): DialogHandle {
  const [order] = useState(() => ++nextOrder);
  const hidden = useContext(DialogsHidden);
  const element = useRef<HTMLDivElement>(null);
  const latest = useRef({ onClose, ...options });
  latest.current = { onClose, ...options };

  useLayoutEffect(() => {
    if (!listening) {
      window.addEventListener("keydown", onKeyDown);
      listening = true;
    }
    entries.set(order, {
      hidden,
      element,
      escape: (e) => {
        const { onEscape, onClose } = latest.current;
        if (onEscape) onEscape(e);
        else onClose();
      },
    });
    return () => void entries.delete(order);
  }, [order, hidden]);

  return useMemo(
    () => ({ hidden, element, isTop: () => topOrder() === order, close: () => latest.current.onClose() }),
    [order, hidden],
  );
}
