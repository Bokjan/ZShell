import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface Props {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  /** Shown below the message, e.g. a preview of what is about to happen. */
  children?: ReactNode;
  /** A checkbox such as "Don't ask again"; its state is passed to `onConfirm`. */
  checkboxLabel?: string;
  onConfirm(checked: boolean): void;
  onCancel(): void;
}

export function ConfirmDialog({ title, message, confirmLabel, danger, children, checkboxLabel, onConfirm, onCancel }: Props) {
  const { t } = useTranslation();
  const [checked, setChecked] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
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
            {t("common.cancel")}
          </button>
          <button type="submit" className={danger ? "primary danger-fill" : "primary"} autoFocus>
            {confirmLabel}
          </button>
        </footer>
      </form>
    </div>
  );
}
