import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { useDialog } from "../lib/dialogs";
import { Modal } from "./Modal";

interface Props {
  title: string;
  message: string;
  confirmLabel: string;
  /** Defaults to "Cancel". */
  cancelLabel?: string;
  danger?: boolean;
  /** Shown below the message, e.g. a preview of what is about to happen. */
  children?: ReactNode;
  /** A checkbox such as "Don't ask again"; its state is passed to `onConfirm`. */
  checkboxLabel?: string;
  /** A second way to go ahead, beside the confirm button. */
  secondaryLabel?: string;
  onSecondary?(): void;
  onConfirm(checked: boolean): void;
  onCancel(): void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  cancelLabel,
  danger,
  children,
  checkboxLabel,
  secondaryLabel,
  onSecondary,
  onConfirm,
  onCancel,
}: Props) {
  const { t } = useTranslation();
  const [checked, setChecked] = useState(false);
  const dialog = useDialog(onCancel);

  return (
    <Modal dialog={dialog}>
      <form
        className="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          onConfirm(checked);
        }}
      >
        <h2>{title}</h2>
        <p className="dialog-message">{message}</p>
        {children}
        {checkboxLabel && (
          <label className="checkbox">
            <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
            {checkboxLabel}
          </label>
        )}
        <footer>
          <span className="grow" />
          <button type="button" onClick={onCancel}>
            {cancelLabel ?? t("common.cancel")}
          </button>
          {secondaryLabel && (
            <button type="button" onClick={onSecondary}>
              {secondaryLabel}
            </button>
          )}
          <button type="submit" className={danger ? "primary danger-fill" : "primary"} autoFocus>
            {confirmLabel}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
