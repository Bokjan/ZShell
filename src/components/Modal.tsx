import type { ReactNode } from "react";
import { createPortal } from "react-dom";

import type { DialogHandle } from "../lib/dialogs";

interface Props {
  dialog: DialogHandle;
  /** Added to the backdrop's, e.g. to place the command palette near the top. */
  className?: string;
  children: ReactNode;
}

/**
 * A dialog's backdrop, over the whole window: pressing on it outside the dialog closes the
 * dialog. Rendered at the end of the body, so that styles of whatever opened it don't apply
 * and later dialogs are drawn above earlier ones.
 */
export function Modal({ dialog, className, children }: Props) {
  return createPortal(
    <div
      className={className ? `dialog-backdrop ${className}` : "dialog-backdrop"}
      hidden={dialog.hidden}
      onMouseDown={(e) => e.target === e.currentTarget && dialog.isTop() && dialog.close()}
    >
      {children}
    </div>,
    document.body,
  );
}
