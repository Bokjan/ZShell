import { createContext, useContext } from "react";

import type { VaultStatus } from "../ipc/api";

/** The vault, as `VaultGate` provides it to the app once it is unlocked. */
export interface VaultContextValue {
  status: VaultStatus;
  /** After the protection changed. */
  setStatus(status: VaultStatus): void;
}

export const VaultContext = createContext<VaultContextValue | null>(null);

export function useVault(): VaultContextValue {
  const value = useContext(VaultContext);
  if (!value) throw new Error("useVault outside VaultGate");
  return value;
}
