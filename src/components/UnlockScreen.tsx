import { useRef, useState, type FormEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { configDirectory, errorMessage, vault, type VaultStatus } from "../lib/api";
import { announce } from "../lib/announce";
import { useDialog } from "../lib/dialogs";
import { matches } from "../lib/keymap";
import { isWindows } from "../lib/platform";
import { useShortcuts } from "../lib/shortcuts";
import { useWindowEvent, windows } from "../lib/windows";
import { ErrorText } from "./ErrorMessage";
import { LockHeading, LockScreen } from "./LockScreen";
import { Modal } from "./Modal";

interface Props {
  /** Locked. */
  status: VaultStatus;
  /** The key is being read from the credential store. */
  trying: boolean;
  /** Reads the file and the key in the credential store again. */
  onTryAgain(): void;
  onStatus(status: VaultStatus): void;
  /** After the vault was deleted: a new one is to be made. */
  onReset(status: VaultStatus): void;
}

/**
 * Covers the window while the sessions are locked (see `VaultGate`): asks for the master
 * password, or says why the key can't be had and what can be done: try again, show the file,
 * or delete everything (reset). Also shown while a new vault is made, and if that fails.
 */
export function UnlockScreen({ status, trying, onTryAgain, onStatus, onReset }: Props) {
  const { t } = useTranslation();
  const lock = status.locked!;
  const store = t(`vault.store.${status.store}`);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resetting, setResetting] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // No tabs to close: closing a tab (⌘W, the File menu's Close) closes the window, as it does without tabs.
  useShortcuts((e) => {
    if (!matches(e, "closeTab")) return false;
    void getCurrentWindow().close().catch(console.error);
    return true;
  });

  // Quitting (⌘Q) asks nothing: until the sessions are unlocked, no window has any.
  useWindowEvent("quit-requested", () => void windows.quit().catch(console.error));

  const asksPassword = lock.reason === "password" || (lock.reason === "saved" && status.protection === "password");

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!password || busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = await vault.unlock(password);
      if (!next.locked) announce(t("vault.unlocked"));
      onStatus(next);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
      requestAnimationFrame(() => inputRef.current?.select());
    }
  };

  const showFile = () =>
    void configDirectory()
      .then((dir) => revealItemInDir(`${dir}${isWindows ? "\\" : "/"}vault.json`))
      .catch(console.error);

  const detail = lock.error ? errorMessage(lock.error) : null;
  // Waiting for the credential store, unless making the vault failed.
  const waiting = lock.reason === "saved" || (lock.reason === "new" && !lock.error);
  let message = t("vault.waiting", { store });
  if (lock.reason === "password") message = t("vault.passwordPrompt");
  else if (lock.reason !== "saved" && !waiting) message = t(`vault.${lock.reason}`, { store });

  return (
    <LockScreen>
      <form className="unlock-panel" onSubmit={submit}>
        <LockHeading />
        <p role="status">{message}</p>
        {lock.reason === "password" && detail && <p className="hint">{t("vault.rememberedFailed", { message: detail })}</p>}
        {lock.reason !== "password" && detail && <p className="hint">{detail}</p>}

        {asksPassword && (
          <>
            <input
              ref={inputRef}
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              aria-label={t("vault.password")}
              placeholder={t("vault.password")}
              autoFocus
              disabled={busy}
            />
            {error && <ErrorText>{error}</ErrorText>}
            <button type="submit" className="primary" disabled={busy || !password}>
              {busy ? t("vault.unlocking") : t("vault.unlock")}
            </button>
            {lock.reason === "password" && (
              <button type="button" className="link" onClick={() => setResetting(true)}>
                {t("vault.forgot")}
              </button>
            )}
          </>
        )}

        {!asksPassword && !waiting && (
          <div className="unlock-actions">
            {(lock.reason === "keyUnavailable" || lock.reason === "unreadable" || lock.reason === "damaged" || lock.reason === "new") && (
              <button type="button" className="primary" onClick={onTryAgain} disabled={trying} autoFocus>
                {t("vault.tryAgain")}
              </button>
            )}
            {(lock.reason === "unreadable" || lock.reason === "damaged" || lock.reason === "newer") && (
              <button type="button" onClick={showFile}>
                {t("vault.showFile")}
              </button>
            )}
            {(lock.reason === "keyUnavailable" || lock.reason === "damaged") && (
              <button type="button" onClick={() => setResetting(true)}>
                {t("vault.reset")}
              </button>
            )}
          </div>
        )}
      </form>
      {resetting && <ResetDialog onClose={() => setResetting(false)} onReset={onReset} />}
    </LockScreen>
  );
}

/**
 * The word typed to confirm deleting the vault, the same in every language: one keyboard
 * layout and no input method needed, like the `yes` / `no` answers to host key prompts.
 */
const RESET_WORD = "DELETE";

/** Deleting the vault, once its typed confirmation matches. */
function ResetDialog({ onClose, onReset }: { onClose(): void; onReset(status: VaultStatus): void }) {
  const { t } = useTranslation();
  const dialog = useDialog(onClose);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (typed.trim() !== RESET_WORD) return;
    setBusy(true);
    try {
      onReset(await vault.reset());
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  return (
    <Modal dialog={dialog} alert>
      <form className="dialog" onSubmit={submit}>
        <h2>{t("vault.resetTitle")}</h2>
        <p className="dialog-message">{t("vault.resetMessage")}</p>
        <label>
          {t("vault.resetWordLabel", { word: RESET_WORD })}
          <input value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus autoComplete="off" />
        </label>
        {error && <ErrorText>{error}</ErrorText>}
        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary danger-fill" disabled={busy || typed.trim() !== RESET_WORD}>
            {t("vault.resetConfirm")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
