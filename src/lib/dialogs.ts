import { createContext, useContext, useLayoutEffect, useMemo, useRef, useState } from "react";

import { isComposing } from "./platform";

/**
 * The open dialogs, as a stack: only the top one answers Escape and clicks on its backdrop,
 * so a dialog opened from another (a proxy from the settings) closes alone. Dialogs are
 * ordered by when they first rendered, which puts one opened from another above it even
 * when both appear at once.
 */
interface Entry {
  hidden: boolean;
  escape(e: KeyboardEvent): void;
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

// Bubble phase, after the focused element: a control that handles Escape itself (a menu, a
// field that cancels an edit) prevents the default to keep the dialog open.
function onKeyDown(e: KeyboardEvent) {
  if (e.key !== "Escape" || e.defaultPrevented || isComposing(e)) return;
  const top = topOrder();
  if (top === null) return;
  e.preventDefault();
  entries.get(top)!.escape(e);
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
}

interface Options {
  /** Instead of closing on Escape, e.g. to clear a search field first. */
  onEscape?(e: KeyboardEvent): void;
}

/** Registers a dialog while it is mounted; render it with `Modal`. */
export function useDialog(onClose: () => void, options: Options = {}): DialogHandle {
  const [order] = useState(() => ++nextOrder);
  const hidden = useContext(DialogsHidden);
  const latest = useRef({ onClose, ...options });
  latest.current = { onClose, ...options };

  useLayoutEffect(() => {
    if (!listening) {
      window.addEventListener("keydown", onKeyDown);
      listening = true;
    }
    entries.set(order, {
      hidden,
      escape: (e) => {
        const { onEscape, onClose } = latest.current;
        if (onEscape) onEscape(e);
        else onClose();
      },
    });
    return () => void entries.delete(order);
  }, [order, hidden]);

  return useMemo(
    () => ({ hidden, isTop: () => topOrder() === order, close: () => latest.current.onClose() }),
    [order, hidden],
  );
}
