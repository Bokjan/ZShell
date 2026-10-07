import { useEffect } from "react";

interface Props {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  onConfirm(): void;
  onCancel(): void;
}

export function ConfirmDialog({ title, message, confirmLabel, danger, onConfirm, onCancel }: Props) {
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
          onConfirm();
        }}
      >
        <h2>{title}</h2>
        <p className="dialog-message">{message}</p>
        <footer>
          <span className="grow" />
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className={danger ? "primary danger-fill" : "primary"} autoFocus>
            {confirmLabel}
          </button>
        </footer>
      </form>
    </div>
  );
}
