import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { appLock, errorCode, errorMessage, type AppLockStatus } from "../ipc/api";
import { setAppLocked } from "../lib/appLock";
import { windows } from "../ipc/windows";
import { listenHere } from "../ipc/events";
import { useConfigChanged } from "../hooks/windowEvents";
import { ErrorText } from "./ErrorMessage";
import { LockHeading, LockScreen } from "./LockScreen";
import { useLatest } from "../hooks/latest";

interface AppLockContextValue {
  status: AppLockStatus;
  /** After the settings changed. */
  setStatus(status: AppLockStatus): void;
  /** The "Lock" action: locks, and returns false if the app lock is off. */
  lock(): boolean;
  /** Closing the window (or quitting) while locked would end busy tabs: asks on the lock screen. */
  confirmClose(close: PendingClose): void;
  /**
   * A close that was waiting on the lock screen when the app was unlocked (the system's dialog
   * can be over it): the close flow asks again, as when unlocked.
   */
  pendingClose: PendingClose | null;
  clearPendingClose(): void;
}

/** Closing the window, or quitting (all of them), would end `busy` tabs. */
export interface PendingClose {
  busy: number;
  quit: boolean;
}

const AppLockContext = createContext<AppLockContextValue | null>(null);

export function useAppLock(): AppLockContextValue {
  const value = useContext(AppLockContext);
  if (!value) throw new Error("useAppLock outside AppLockGate");
  return value;
}

/** Whether the app lock can be used and is on. */
export const appLockOn = (status: AppLockStatus) => !!status.settings?.enabled && status.method !== null;

/**
 * Covers the app with the lock screen while the app lock is locked (see `applock/mod.rs`),
 * keeping it mounted: the sessions keep running. Inside `VaultGate`, since the settings are
 * in the vault; nothing shows until the backend has said whether it is locked, so the app
 * never flashes before the lock screen at startup.
 */
export function AppLockGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AppLockStatus | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // Sessions that closing the window would end, waiting for confirmation on the lock screen;
  // left for the close flow once unlocked.
  const [closing, setClosing] = useState<PendingClose | null>(null);
  // Locked by the user in this window: they are about to leave, so the system isn't asked
  // until the window comes back.
  const [byHand, setByHand] = useState(false);
  const statusRef = useLatest(status);

  const refresh = useCallback(() => void appLock.status().then(setStatus, (e) => setFailure(errorMessage(e))), []);

  // Covered at once, then the rest of the status: asking Windows Hello what it offers takes a
  // moment, which the sessions would show through.
  const coverNow = useCallback(() => setStatus((current) => current && { ...current, locked: true }), []);

  // Locked by the backend (idle, the system locking or sleeping).
  useEffect(() => {
    refresh();
    const unlisten = listenHere("app-locked", () => {
      coverNow();
      refresh();
    });
    return () => void unlisten.then((f) => f());
  }, [refresh, coverNow]);
  // Unlocked in another window, or its settings changed there.
  useConfigChanged("appLock", refresh);

  const locked = status?.locked ?? false;
  useLayoutEffect(() => setAppLocked(locked), [locked]);
  useEffect(() => {
    if (!locked) setByHand(false);
  }, [locked]);

  const lock = useCallback(() => {
    const current = statusRef.current;
    if (!current || !appLockOn(current)) return false;
    setByHand(true);
    coverNow();
    void appLock.lock().then(setStatus, console.error);
    return true;
  }, [coverNow]);
  const confirmClose = useCallback((close: PendingClose) => setClosing(close), []);
  const clearPendingClose = useCallback(() => setClosing(null), []);

  if (!status) return failure ? <p className="vault-failure">{failure}</p> : null;
  return (
    <AppLockContext.Provider value={{ status, setStatus, lock, confirmClose, pendingClose: locked ? null : closing, clearPendingClose }}>
      {children}
      {locked && (
        <AppLockScreen status={status} byHand={byHand} onStatus={setStatus} closing={closing} onKeepOpen={() => setClosing(null)} />
      )}
    </AppLockContext.Provider>
  );
}

interface ScreenProps {
  status: AppLockStatus;
  /** Locked with the Lock action in this window: the system is asked once the window comes back. */
  byHand: boolean;
  onStatus(status: AppLockStatus): void;
  closing: PendingClose | null;
  onKeepOpen(): void;
}

/**
 * A modal `<dialog>` in the top layer: above everything, including dialogs, menus and tooltips
 * rendered at the end of the body, and everything else in the page is inert, for the mouse,
 * the keyboard and screen readers alike. It has its own title bar, since the window's is
 * inert.
 *
 * It asks the system by itself once, when the window has the focus: locking while the user is
 * in another app mustn't put the system's dialog in front of them. Locked by hand, it waits for
 * the window to get the focus again, since the user is leaving. After they cancel, only the
 * button asks. When there is no way to authenticate left (a Windows Hello PIN removed), it says
 * so, and only a button of its own unlocks without.
 */
function AppLockScreen({ status, byHand, onStatus, closing, onKeepOpen }: ScreenProps) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Why unlocking can't ask, once the backend said so.
  const [gone, setGone] = useState<string | null>(null);
  const asking = useRef(false);
  const asked = useRef(false);

  useLayoutEffect(() => {
    const dialog = ref.current!;
    // Escape would close it: the cancel event is prevented, but a browser may close the
    // dialog all the same when it is pressed again, so it is opened again.
    const cancel = (e: Event) => e.preventDefault();
    const reopen = () => !dialog.open && dialog.showModal();
    dialog.addEventListener("cancel", cancel);
    dialog.addEventListener("close", reopen);
    dialog.showModal();
    // React focuses an `autoFocus` element before this effect, while the dialog is still
    // closed, so that fails; and `showModal` focuses the first button inside, the window's
    // minimize button on Windows and Linux. The panel's first button is the one to focus.
    dialog.querySelector<HTMLElement>(".unlock-panel button")?.focus();
    return () => {
      dialog.removeEventListener("cancel", cancel);
      dialog.removeEventListener("close", reopen);
      dialog.close();
    };
  }, []);

  const unlock = useCallback(
    async (anyway = false) => {
      if (asking.current) return;
      asking.current = asked.current = true;
      setBusy(true);
      setError(null);
      try {
        onStatus(await appLock.unlock(anyway));
      } catch (e) {
        const code = errorCode(e);
        if (code === "appLock.gone") setGone(errorMessage(e));
        else if (code !== "appLock.cancelled" && code !== "appLock.busy") setError(errorMessage(e));
      } finally {
        asking.current = false;
        setBusy(false);
      }
    },
    [onStatus],
  );

  // The window's focus, not `document.hasFocus()`: WKWebView says the page has it while the
  // app is in the background.
  // Read once: the lock screen asks at once or waits from the start.
  const waitForFocus = useRef(byHand);
  useEffect(() => {
    const window = getCurrentWindow();
    const ask = () => !asked.current && void unlock();
    if (!waitForFocus.current) void window.isFocused().then((focused) => focused && ask(), console.error);
    const unlisten = window.onFocusChanged(({ payload: focused }) => focused && ask());
    return () => void unlisten.then((f) => f());
  }, [unlock]);

  const message = status.method ?? "password";

  return (
    <dialog ref={ref} className="app-lock" aria-labelledby="app-lock-title" onKeyDown={(e) => e.key === "Escape" && e.preventDefault()}>
      <LockScreen>
        {closing === null ? (
          <div className="unlock-panel">
            <LockHeading id="app-lock-title" />
            {gone === null ? (
              <>
                <p>{t(`appLock.prompt.${message}`)}</p>
                {error && <ErrorText>{error}</ErrorText>}
                <button type="button" className="primary" onClick={() => void unlock()} disabled={busy} autoFocus>
                  {busy ? t("vault.unlocking") : t("vault.unlock")}
                </button>
              </>
            ) : (
              <>
                <div role="alert">
                  <p>{gone}</p>
                  <p>{t("appLock.goneHint")}</p>
                </div>
                {error && <ErrorText>{error}</ErrorText>}
                <button type="button" className="primary" onClick={() => void unlock(true)} disabled={busy} autoFocus>
                  {busy ? t("vault.unlocking") : t("appLock.unlockAnyway")}
                </button>
              </>
            )}
          </div>
        ) : (
          <div className="unlock-panel" role="alert">
            <LockHeading id="app-lock-title" />
            <p>{t(closing.quit ? "closeConfirm.quit" : "closeConfirm.window", { count: closing.busy })}</p>
            <div className="unlock-actions">
              <button type="button" onClick={onKeepOpen} autoFocus>
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className="primary danger-fill"
                onClick={() => void (closing.quit ? windows.quit() : getCurrentWindow().destroy())}
              >
                {t(closing.quit ? "closeConfirm.quitConfirm" : "closeConfirm.windowConfirm")}
              </button>
            </div>
          </div>
        )}
      </LockScreen>
    </dialog>
  );
}
