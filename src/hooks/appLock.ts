import { createContext, useContext } from "react";

import type { AppLockStatus } from "../ipc/api";

/** The app lock, as `AppLockGate` provides it to the app it covers. */
export interface AppLockContextValue {
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

export const AppLockContext = createContext<AppLockContextValue | null>(null);

export function useAppLock(): AppLockContextValue {
  const value = useContext(AppLockContext);
  if (!value) throw new Error("useAppLock outside AppLockGate");
  return value;
}

/** Whether the app lock can be used and is on. */
export const appLockOn = (status: AppLockStatus) => !!status.settings?.enabled && status.method !== null;
