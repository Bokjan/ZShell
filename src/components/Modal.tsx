import { useEffect, useId, useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { focusables, type DialogHandle } from "../hooks/dialogs";
import { useShortcuts } from "../hooks/shortcuts";
import { matches } from "../lib/keymap";

interface Props {
  dialog: DialogHandle;
  /** For a dialog without a title (`h2`), which otherwise names it. */
  label?: string;
  /**
   * A question that interrupts (`alertdialog`): screen readers also read its message, the
   * `.dialog-message` element.
   */
  alert?: boolean;
  /** Added to the backdrop's, e.g. to place the command palette near the top. */
  className?: string;
  children: ReactNode;
}

/**
 * A dialog's backdrop, over the whole window: pressing on it outside the dialog closes a
 * `dismissable` dialog, and leaves any other as it is, the focus included. Rendered at the
 * end of the body, so that styles of whatever opened it don't apply and later dialogs are
 * drawn above earlier ones.
 *
 * For screen readers it is the modal dialog, named by its title. It takes the focus when it
 * opens (unless a control in it already has it, `autoFocus`), Tab stays within it (see
 * `useDialog`), and the focus goes back to where it was once it closes.
 *
 * Closing a tab (⌘W, the File menu's Close; Ctrl+Shift+W elsewhere) closes the top dialog
 * instead, as its Cancel button does.
 */
export function Modal({ dialog, label, alert, className, children }: Props) {
  const id = useId();
  // Before the dialog's controls take the focus.
  const [opener] = useState(() => document.activeElement);

  useShortcuts(
    (e) => {
      if (!matches(e, "closeTab")) return false;
      dialog.close();
      return true;
    },
    { dialog },
  );

  // Names the dialog after its title, and describes a question with its message.
  useLayoutEffect(() => {
    const element = dialog.element.current;
    if (!element || label) return;
    const title = element.querySelector("h2");
    if (title) {
      if (!title.id) title.id = `${id}-title`;
      element.setAttribute("aria-labelledby", title.id);
    }
    const message = alert ? element.querySelector(".dialog-message") : null;
    if (message) {
      if (!message.id) message.id = `${id}-message`;
      element.setAttribute("aria-describedby", message.id);
    }
  }, [dialog, label, alert, id]);

  useEffect(() => {
    const element = dialog.element.current;
    if (element && !dialog.hidden && !element.contains(document.activeElement)) (focusables(element)[0] ?? element).focus();
    return () => {
      // After whoever closed it has moved the focus, if they did (to the terminal, say).
      requestAnimationFrame(() => {
        const lost = !document.activeElement || document.activeElement === document.body;
        if (lost && opener instanceof HTMLElement && opener.isConnected) opener.focus({ preventScroll: true });
      });
    };
  }, [dialog, opener]);

  // Taken apart: React Compiler treats an object holding a ref as a ref (see `useLatest`).
  const { element, hidden } = dialog;
  return createPortal(
    <div
      ref={element}
      className={className ? `dialog-backdrop ${className}` : "dialog-backdrop"}
      role={alert ? "alertdialog" : "dialog"}
      aria-modal="true"
      aria-label={label}
      // Focusable by script only, for a dialog without controls.
      tabIndex={-1}
      hidden={hidden}
      onMouseDown={(e) => {
        if (e.target !== e.currentTarget) return;
        e.preventDefault();
        if (dialog.isTop()) dialog.dismiss();
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
