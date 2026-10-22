import { useEffect } from "react";

import { quickCommands } from "../ipc/api";
import { quickCommandStore } from "../lib/quickCommands";
import { useConfigChanged } from "../hooks/windowEvents";

/**
 * Keeps the quick commands (`quickCommandStore`) up to date while the app is shown: read when
 * it mounts, which is once the vault is unlocked, again when another window changes them, and
 * forgotten when it unmounts.
 */
export function useQuickCommandSync() {
  useEffect(() => {
    let shown = true;
    quickCommands
      .get()
      .then((commands) => shown && quickCommandStore.loaded(commands))
      .catch(console.error);
    return () => {
      shown = false;
      quickCommandStore.clear();
    };
  }, []);
  useConfigChanged("quickCommands", quickCommandStore.reload);
}
