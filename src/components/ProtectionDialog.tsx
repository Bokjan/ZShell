import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorMessage, vault, type Protection, type ProtectionChange, type VaultStatus } from "../ipc/api";
import { announce } from "../lib/announce";
import { useDialog } from "../hooks/dialogs";
import { attempt } from "../lib/attempt";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";
import { PasswordInput } from "./PasswordInput";

interface Props {
  /**
   * Shown once, over the app, when the vault was just made with a protection chosen without
   * asking: says which, and "Keep" instead of "Cancel".
   */
  intro?: boolean;
  /** Unlocked. */
  status: VaultStatus;
  onDone(status: VaultStatus): void;
  onClose(): void;
}

/**
 * How the sessions and passwords are protected: with a master password (perhaps remembered
 * in the credential store), with a key in the credential store, or not at all. Each choice
 * says what it protects from and what it doesn't. Leaving or changing a master password takes
 * the current one.
 */
export function ProtectionDialog({ intro, status, onDone, onClose }: Props) {
  const { t } = useTranslation();
  const dialog = useDialog(onClose);
  // Unlocked, so known.
  const current = status.protection!;
  const store = t(`vault.store.${status.store}`);
  const [choice, setChoice] = useState<Protection>(current);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [currentPassword, setCurrentPassword] = useState("");
  const [changePassword, setChangePassword] = useState(false);
  const [remember, setRemember] = useState(status.remember);
  // Whether the credential store can be used; null while checking (Linux may have none).
  const [storeAvailable, setStoreAvailable] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    vault.storeAvailable().then(setStoreAvailable, () => setStoreAvailable(false));
  }, []);

  // A new master password is asked for unless one is kept.
  const newPassword = choice === "password" && (current !== "password" || changePassword);
  const usesStore = choice === "system" || (choice === "password" && remember);
  const unchanged = choice === current && (choice !== "password" || (!changePassword && remember === status.remember));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    if (newPassword && !password) return setError(t("protection.passwordEmpty"));
    if (newPassword && password !== confirm) return setError(t("protection.passwordsDontMatch"));
    if (unchanged) return onClose();
    const change: ProtectionChange =
      choice === "password" ? { protection: "password", password: newPassword ? password : null, remember } : { protection: choice };
    setBusy(true);
    return attempt(
      async () => {
        const next = await vault.setProtection(change, current === "password" ? currentPassword : null);
        announce(t("protection.changed"));
        onDone(next);
      },
      {
        onError: (err) => {
          setError(errorMessage(err));
          setBusy(false);
        },
      },
    );
  };

  const option = (value: Protection, label: string, description: string, disabled = false) => (
    <label className={disabled ? "protection-option disabled" : "protection-option"}>
      <input
        type="radio"
        name="protection"
        value={value}
        checked={choice === value}
        disabled={disabled || busy}
        onChange={() => {
          setChoice(value);
          setError(null);
        }}
      />
      <span>
        <span className="protection-label">{label}</span>
        <span className="protection-description">{description}</span>
      </span>
    </label>
  );

  return (
    <Modal dialog={dialog}>
      <form className="dialog protection-dialog" onSubmit={submit}>
        <h2>{t(intro ? "protection.introTitle" : "protection.title")}</h2>
        {intro && (
          <p className="dialog-message">{t(current === "system" ? "protection.introSystem" : "protection.introNone", { store })}</p>
        )}

        <fieldset disabled={busy}>
          <legend className="visually-hidden">{t("protection.choice")}</legend>
          {option("password", t("protection.password"), t("protection.passwordDescription"))}
          {option(
            "system",
            t("protection.system", { store }),
            storeAvailable === false
              ? t("protection.systemUnavailable", { store })
              : storeAvailable === null && status.store === "secretService"
                ? t("protection.checking", { store })
                : t(status.store === "keychain" ? "protection.systemDescription" : "protection.systemDescriptionShared"),
            storeAvailable === false && current !== "system",
          )}
          {option("none", t("protection.none"), t("protection.noneDescription"))}
        </fieldset>

        {current === "password" && !unchanged && (
          <PasswordInput
            label={t("protection.currentPassword")}
            value={currentPassword}
            onChange={(e) => setCurrentPassword(e.target.value)}
            disabled={busy}
          />
        )}

        {choice === "password" && (
          <>
            {current === "password" && (
              <label className="checkbox">
                <input type="checkbox" checked={changePassword} onChange={(e) => setChangePassword(e.target.checked)} disabled={busy} />
                {t("protection.changePassword")}
              </label>
            )}
            {newPassword && (
              <>
                <PasswordInput
                  label={t("protection.newPassword")}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  disabled={busy}
                />
                <PasswordInput
                  label={t("protection.confirmPassword")}
                  value={confirm}
                  onChange={(e) => setConfirm(e.target.value)}
                  disabled={busy}
                />
              </>
            )}
            <label className="checkbox">
              <input
                type="checkbox"
                checked={remember}
                onChange={(e) => setRemember(e.target.checked)}
                disabled={busy || (storeAvailable === false && !status.remember)}
              />
              {t("protection.remember")}
            </label>
            <p className="hint">{t("protection.rememberHint", { store })}</p>
          </>
        )}

        {usesStore && storeAvailable === false && <p className="hint warning">{t("protection.systemUnavailable", { store })}</p>}

        {error && <ErrorText>{error}</ErrorText>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose} disabled={busy}>
            {t(intro ? "protection.keep" : "common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={busy}>
            {busy ? t("protection.applying") : t("protection.apply")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
