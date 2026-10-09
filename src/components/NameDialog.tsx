import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { isComposing } from "../lib/platform";

interface Props {
  title: string;
  label: string;
  initial: string;
  onSave(name: string): void;
  onClose(): void;
}

/** Asks for a name, e.g. of a quick command group. */
export function NameDialog({ title, label, initial, onSave, onClose }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState(initial);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !isComposing(e) && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    onSave(name.trim());
    onClose();
  };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog" onSubmit={submit}>
        <h2>{title}</h2>
        <label>
          {label}
          <input value={name} onChange={(e) => setName(e.target.value)} autoFocus onFocus={(e) => e.target.select()} />
        </label>
        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={!name.trim()}>
            {t("common.save")}
          </button>
        </footer>
      </form>
    </div>
  );
}
