import { useSyncExternalStore } from "react";

import { localShells, type FoundShell } from "../ipc/api";

/**
 * The shells found here, as last heard from the backend, which keeps them for every window;
 * `allowed` is false where the system blocks local terminals (Windows in S mode). Null until
 * heard.
 */
let found: { allowed: boolean; shells: FoundShell[] } | null = null;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  if (found === null) void loadLocalShells();
  return () => void listeners.delete(listener);
};

/**
 * Asks the backend for the shells found here: those it keeps (another window may have found
 * them again since), or, if `refresh`, found again now.
 */
export function loadLocalShells(refresh = false): Promise<void> {
  if (loading && !refresh) return loading;
  const load: Promise<void> = localShells(refresh)
    .then(
      (shells) => {
        found = { allowed: shells !== null, shells: shells ?? [] };
        for (const listener of listeners) listener();
      },
      (e: unknown) => console.error(e),
    )
    .finally(() => {
      if (loading === load) loading = null;
    });
  loading = load;
  return load;
}

/** The shells found here (see `pty/discover.rs`); null until known. */
export const useLocalShells = (): FoundShell[] | null => useSyncExternalStore(subscribe, () => found?.shells ?? null);

/** Whether the system lets ZShell open local terminals; true until known. */
export const useLocalAllowed = (): boolean => useSyncExternalStore(subscribe, () => found?.allowed ?? true);
