import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";

import { errorMessage, vault, type VaultStatus } from "../lib/api";
import { useSettings } from "../lib/settings";
import { useTitleBar } from "../lib/window";
import { useConfigChanged } from "../lib/windows";
import { ProtectionDialog } from "./ProtectionDialog";
import { UnlockScreen } from "./UnlockScreen";

interface VaultContextValue {
  status: VaultStatus;
  /** After the protection changed. */
  setStatus(status: VaultStatus): void;
}

const VaultContext = createContext<VaultContextValue | null>(null);

export function useVault(): VaultContextValue {
  const value = useContext(VaultContext);
  if (!value) throw new Error("useVault outside VaultGate");
  return value;
}

/**
 * Shows `children` (the app) once the sessions can be read: until the vault is unlocked, the
 * unlock screen covers the window, and no session or tab exists. The key in the credential
 * store is tried at once; it may take a while (macOS asking for permission, Linux unlocking
 * the keyring), which the screen says.
 *
 * Asks once, over the app, how to protect the sessions while that hasn't been chosen.
 */
export function VaultGate({ children }: { children: ReactNode }) {
  const { settings, update } = useSettings();
  // The title bar works on the unlock screen too: moving the window, its buttons.
  useTitleBar();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // Reading the key from the credential store.
  const [trying, setTrying] = useState(false);

  const tryUnlock = useCallback(() => {
    setTrying(true);
    vault
      .tryUnlock()
      .then(setStatus, (e) => setFailure(errorMessage(e)))
      .finally(() => setTrying(false));
  }, []);

  useEffect(() => {
    vault.status().then(
      (found) => {
        setStatus(found);
        if (found.locked?.reason === "saved") tryUnlock();
      },
      (e) => setFailure(errorMessage(e)),
    );
  }, [tryUnlock]);

  // Its protection changed in another window.
  useConfigChanged("vault", () => void vault.status().then(setStatus, console.error));

  // Reset: everything starts again, including the question.
  const onReset = (next: VaultStatus) => {
    update({ ...settings, security: { ...settings.security, prompted: false } });
    setStatus(next);
  };

  if (!status) return failure ? <p className="vault-failure">{failure}</p> : null;
  if (status.locked) {
    return <UnlockScreen status={status} trying={trying} onTryAgain={tryUnlock} onStatus={setStatus} onReset={onReset} />;
  }
  const asking = status.protection === "unset" && !settings.security.prompted;
  const prompted = () => update({ ...settings, security: { ...settings.security, prompted: true } });
  return (
    <VaultContext.Provider value={{ status, setStatus }}>
      {children}
      {asking && (
        <ProtectionDialog
          onboarding
          status={status}
          onDone={(next) => {
            setStatus(next);
            prompted();
          }}
          onClose={prompted}
        />
      )}
    </VaultContext.Provider>
  );
}
