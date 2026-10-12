import { useEffect, useRef, useState, type FormEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { configDirectory, errorMessage, vault, type VaultStatus } from "../lib/api";
import { announce } from "../lib/announce";
import { useDialog } from "../lib/dialogs";
import { isWindows } from "../lib/platform";
import { DRAG_REGION } from "../lib/window";
import { ErrorText } from "./ErrorMessage";
import { LockIcon } from "./icons";
import { Modal } from "./Modal";
import { WindowControls } from "./WindowControls";

interface Props {
  /** Locked. */
  status: VaultStatus;
  /** The key is being read from the credential store. */
  trying: boolean;
  /** Reads the file and the key in the credential store again. */
  onTryAgain(): void;
  onStatus(status: VaultStatus): void;
  /** After the vault was deleted. */
  onReset(status: VaultStatus): void;
}

/**
 * Covers the window while the sessions are locked (see `VaultGate`): asks for the master
 * password, or says why the key can't be had and what can be done: try again, show the file,
 * or delete everything (reset). M26's app lock is to reuse it.
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

  // No tabs to close: ⌘W (the File menu's Close) closes the window, as it does without tabs.
  useEffect(() => {
    const unlisten = listen("close-tab", () => void getCurrentWindow().close().catch(console.error));
    return () => void unlisten.then((f) => f());
  }, []);

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
  let message: string;
  if (lock.reason === "saved") message = t("vault.waiting", { store });
  else if (lock.reason === "password") message = t("vault.passwordPrompt");
  else message = t(`vault.${lock.reason}`, { store });

  return (
    <div className="unlock-screen">
      <div className="unlock-titlebar" {...DRAG_REGION}>
        {isWindows && <WindowControls />}
      </div>
      <main className="unlock-body">
        <form className="unlock-panel" onSubmit={submit}>
          <span className="unlock-icon">
            <LockIcon size={36} />
          </span>
          <h1>{t("vault.title")}</h1>
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

          {!asksPassword && lock.reason !== "saved" && (
            <div className="unlock-actions">
              {(lock.reason === "keyUnavailable" || lock.reason === "unreadable") && (
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
      </main>
      {resetting && <ResetDialog onClose={() => setResetting(false)} onReset={onReset} />}
    </div>
  );
}

/** Deleting the vault, once its typed confirmation matches. */
function ResetDialog({ onClose, onReset }: { onClose(): void; onReset(status: VaultStatus): void }) {
  const { t } = useTranslation();
  const dialog = useDialog(onClose);
  const word = t("vault.resetWord");
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (typed.trim() !== word) return;
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
          {t("vault.resetWordLabel", { word })}
          <input value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus autoComplete="off" spellCheck={false} />
        </label>
        {error && <ErrorText>{error}</ErrorText>}
        <footer>
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary danger-fill" disabled={busy || typed.trim() !== word}>
            {t("vault.resetConfirm")}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
