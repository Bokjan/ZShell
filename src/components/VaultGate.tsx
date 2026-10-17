import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

import { errorMessage, vault, type VaultStatus } from "../ipc/api";
import { useNativeMenu } from "../hooks/appMenu";
import { useTitleBar } from "../hooks/titleBar";
import { useConfigChanged } from "../hooks/windowEvents";
import { VaultContext } from "../hooks/vault";
import { latestOnly } from "../lib/latestOnly";
import { ProtectionDialog } from "./ProtectionDialog";
import { UnlockScreen } from "./UnlockScreen";

/**
 * Shows `children` (the app) once the sessions can be read: until the vault is unlocked, the
 * unlock screen covers the window, and no session or tab exists. The key in the credential
 * store is tried at once, and a missing vault made; it may take a while (macOS asking for
 * permission, Linux unlocking the keyring), which the screen says. The window that made the
 * vault then says how it is protected, and offers to change it.
 */
export function VaultGate({ children }: { children: ReactNode }) {
  // The title bar works on the unlock screen too: moving the window, its buttons; and so
  // does the macOS menu bar.
  useTitleBar();
  useNativeMenu();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // Reading the key from the credential store, or making the vault.
  const [trying, setTrying] = useState(false);
  // The vault was just made here, protected without asking: says how, once.
  const [introducing, setIntroducing] = useState(false);
  // Read again as other windows change it: only the latest answer counts (see `latestOnly`).
  const statuses = useMemo(() => latestOnly<VaultStatus>(setStatus), []);

  const tryUnlock = useCallback(() => {
    setTrying(true);
    statuses
      .track(vault.tryUnlock())
      .then(
        (next) => {
          if (next.created) setIntroducing(true);
        },
        (e) => setFailure(errorMessage(e)),
      )
      .finally(() => setTrying(false));
  }, [statuses]);

  // Read at once: a saved key, or no vault yet.
  const found = useCallback(
    (next: VaultStatus) => {
      statuses.changed(next);
      if (next.locked?.reason === "saved" || (next.locked?.reason === "new" && !next.locked.error)) tryUnlock();
    },
    [tryUnlock, statuses],
  );

  useEffect(() => {
    vault.status().then(found, (e) => setFailure(errorMessage(e)));
  }, [found]);

  // Its protection changed in another window.
  useConfigChanged("vault", () => void statuses.track(vault.status()).catch(console.error));

  if (!status) return failure ? <p className="vault-failure">{failure}</p> : null;
  if (status.locked) {
    return <UnlockScreen status={status} trying={trying} onTryAgain={tryUnlock} onStatus={statuses.changed} onReset={found} />;
  }
  return (
    <VaultContext.Provider value={{ status, setStatus: statuses.changed }}>
      {children}
      {introducing && (
        <ProtectionDialog
          intro
          status={status}
          onDone={(next) => {
            statuses.changed(next);
            setIntroducing(false);
          }}
          onClose={() => setIntroducing(false)}
        />
      )}
    </VaultContext.Provider>
  );
}
