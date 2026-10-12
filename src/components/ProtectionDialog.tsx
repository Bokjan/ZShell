import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { errorCode, errorMessage, vault, type Protection, type ProtectionChange, type VaultStatus } from "../lib/api";
import { announce } from "../lib/announce";
import { useDialog } from "../lib/dialogs";
import { ErrorText } from "./ErrorMessage";
import { Modal } from "./Modal";

type Choice = Exclude<Protection, "unset">;

interface Props {
  /**
   * Asked once, at the first start: "Later" instead of "Cancel", an introduction, and only
   * the buttons close it.
   */
  onboarding?: boolean;
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
export function ProtectionDialog({ onboarding, status, onDone, onClose }: Props) {
  const { t } = useTranslation();
  // Asked once: answered with "Later" or "Apply", not dismissed by accident.
  const dialog = useDialog(() => close(), { persistent: onboarding });
  const current = status.protection ?? "unset";
  const store = t(`vault.store.${status.store}`);
  const [choice, setChoice] = useState<Choice>(current === "unset" ? "password" : current);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [currentPassword, setCurrentPassword] = useState("");
  const [changePassword, setChangePassword] = useState(false);
  const [remember, setRemember] = useState(status.remember);
  // Whether the credential store can be used; null while checking (Linux may have none).
  const [storeAvailable, setStoreAvailable] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Passwords that couldn't be read from the credential store, which may be left behind.
  const [unreadable, setUnreadable] = useState(false);
  // Changed, but the old files couldn't all be deleted (`vault.leftover`): the new status,
  // shown with that error until the dialog is closed.
  const [changed, setChanged] = useState<VaultStatus | null>(null);
  // Changed to encryption, with unreadable plain files set aside earlier still in the clear:
  // the new status and their names, until the user deletes or keeps them.
  const [copies, setCopies] = useState<{ next: VaultStatus; names: string[] } | null>(null);

  useEffect(() => {
    vault.storeAvailable().then(setStoreAvailable, () => setStoreAvailable(false));
  }, []);

  // A new master password is asked for unless one is kept.
  const newPassword = choice === "password" && (current !== "password" || changePassword);
  const usesStore = choice === "system" || (choice === "password" && remember);
  const unchanged =
    choice === current && (choice !== "password" || (!changePassword && remember === status.remember));

  const apply = async (skipUnreadable: boolean) => {
    setError(null);
    if (newPassword && !password) return setError(t("protection.passwordEmpty"));
    if (newPassword && password !== confirm) return setError(t("protection.passwordsDontMatch"));
    if (unchanged) return onboarding ? onDone(status) : onClose();
    const change: ProtectionChange =
      choice === "password"
        ? { protection: "password", password: newPassword ? password : null, remember }
        : { protection: choice };
    setBusy(true);
    try {
      const next = await vault.setProtection(change, current === "password" ? currentPassword : null, skipUnreadable);
      announce(t("protection.changed"));
      const names = choice === "none" ? [] : await vault.plainCopies().catch(() => []);
      if (names.length > 0) {
        setBusy(false);
        return setCopies({ next, names });
      }
      onDone(next);
    } catch (err) {
      if (errorCode(err) === "vault.leftover") {
        announce(t("protection.changed"));
        setChanged(await vault.status().catch(() => status));
      }
      setUnreadable(errorCode(err) === "vault.passwordsUnreadable");
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  // Once changed, closing passes the new status on.
  const close = () => (changed ? onDone(changed) : copies ? onDone(copies.next) : onClose());

  const deleteCopies = async () => {
    if (!copies) return;
    setError(null);
    setBusy(true);
    try {
      await vault.deletePlainCopies();
      onDone(copies.next);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (changed) close();
    else if (copies) void deleteCopies();
    else void apply(false);
  };

  if (copies) {
    return (
      <Modal dialog={dialog}>
        <form className="dialog protection-dialog" onSubmit={submit}>
          <h2>{t("protection.copiesTitle")}</h2>
          <p className="dialog-message">{t("protection.copiesMessage")}</p>
          <ul className="set-aside-list">
            {copies.names.map((name) => (
              <li key={name}>{name}</li>
            ))}
          </ul>
          {error && <ErrorText>{error}</ErrorText>}
          <footer>
            <span className="grow" />
            <button type="button" onClick={close} disabled={busy}>
              {t("protection.copiesKeep")}
            </button>
            <button type="submit" className="primary danger-fill" disabled={busy} autoFocus>
              {t("common.delete")}
            </button>
          </footer>
        </form>
      </Modal>
    );
  }

  if (changed) {
    return (
      <Modal dialog={dialog}>
        <form className="dialog protection-dialog" onSubmit={submit}>
          <h2>{t(onboarding ? "protection.onboardingTitle" : "protection.title")}</h2>
          {error && <ErrorText>{error}</ErrorText>}
          <footer>
            <span className="grow" />
            <button type="submit" className="primary" autoFocus>
              {t("common.close")}
            </button>
          </footer>
        </form>
      </Modal>
    );
  }

  const option = (value: Choice, label: string, description: string, disabled = false) => (
    <label className={disabled ? "protection-option disabled" : "protection-option"}>
      <input
        type="radio"
        name="protection"
        value={value}
        checked={choice === value}
        disabled={disabled || busy}
        onChange={() => {
          setChoice(value);
          setUnreadable(false);
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
        <h2>{t(onboarding ? "protection.onboardingTitle" : "protection.title")}</h2>
        {onboarding && <p className="dialog-message">{t("protection.onboardingIntro")}</p>}

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
          <label>
            {t("protection.currentPassword")}
            <input
              type="password"
              value={currentPassword}
              onChange={(e) => setCurrentPassword(e.target.value)}
              disabled={busy}
            />
          </label>
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
                <label>
                  {t("protection.newPassword")}
                  <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} disabled={busy} />
                </label>
                <label>
                  {t("protection.confirmPassword")}
                  <input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} disabled={busy} />
                </label>
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

        {!unchanged && choice !== "none" && current !== "system" && current !== "password" && (
          <p className="hint">{t("protection.olderVersions")}</p>
        )}
        {!unchanged && choice === "none" && <p className="hint">{t("protection.olderVersionsNone")}</p>}
        {usesStore && storeAvailable === false && <p className="hint warning">{t("protection.systemUnavailable", { store })}</p>}

        {error && <ErrorText>{error}</ErrorText>}
        {unreadable && <p className="hint">{t("protection.unreadableHint", { store })}</p>}

        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose} disabled={busy}>
            {t(onboarding ? "protection.later" : "common.cancel")}
          </button>
          {unreadable && (
            <button type="button" onClick={() => void apply(true)} disabled={busy}>
              {t("protection.skipUnreadable")}
            </button>
          )}
          <button type="submit" className="primary" disabled={busy}>
            {busy ? t("protection.applying") : t("protection.apply")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
