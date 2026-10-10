import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface Props {
  value: string;
  onChange(value: string): void;
  /** Whether the stored password is to be deleted (the box below), which disables the field. */
  clear: boolean;
  onClearChange(clear: boolean): void;
  /** Whether there may be a stored one to delete: something saved is being edited. */
  canClear: boolean;
  placeholder: string;
  hint?: ReactNode;
}

/** A password to store in the system keychain, with the choice to delete the stored one. */
export function PasswordField({ value, onChange, clear, onClearChange, canClear, placeholder, hint }: Props) {
  const { t } = useTranslation();
  return (
    <>
      <label>
        {t("profile.password")}
        <input type="password" value={value} onChange={(e) => onChange(e.target.value)} disabled={clear} placeholder={placeholder} />
      </label>
      {hint && <p className="hint">{hint}</p>}
      {canClear && (
        <label className="checkbox">
          <input type="checkbox" checked={clear} onChange={(e) => onClearChange(e.target.checked)} />
          {t("profile.clearPassword")}
        </label>
      )}
    </>
  );
}
