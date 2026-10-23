import { useSyncExternalStore } from "react";

import { directory } from "../lib/directory";
import { quickCommandStore } from "../lib/quickCommands";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { TabStore } from "../lib/tabs";

/** The tabs of `store`, rendering again when they change. */
export const useTabStore = (store: TabStore) => useSyncExternalStore(store.subscribe, store.get);

/** The session directory, rendering again when it changes. */
export const useDirectory = () => useSyncExternalStore(directory.subscribe, directory.get);

/**
 * The quick commands (null until read) and why the last change to them couldn't be saved,
 * rendering again when they change. Changes are saved with `quickCommandStore.save`.
 */
export function useQuickCommands() {
  const { value, saveError } = useSyncExternalStore(quickCommandStore.subscribe, quickCommandStore.get);
  return { commands: value, saveError };
}

/**
 * What the registry has for a pane (its session, its terminal), rendering again when that
 * comes or goes: a new pane's terminal attaches its session in an effect, after whatever
 * shows it has rendered, and a terminal shown again attaches a new one.
 */
export function useRegistered<T>(sessions: SessionRegistry, read: () => T): T {
  return useSyncExternalStore(sessions.subscribe, read);
}
