import { useEffect } from "react";

import { directory } from "../lib/directory";
import { RECENT_KEY } from "../lib/sessions";
import { useConfigChanged } from "../hooks/windowEvents";

/**
 * Keeps the session directory (`lib/directory.ts`) up to date while the app is shown: read
 * when it mounts, which is once the vault is unlocked, again when another window changes it,
 * and forgotten when it unmounts.
 */
export function useDirectorySync() {
  useEffect(() => {
    void directory.reload();
    directory.recentChanged();
    return directory.clear;
  }, []);
  useConfigChanged("profiles", () => void directory.reload());
  // Opened in another window (the windows share the browser storage, and hear its changes).
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === null || e.key === RECENT_KEY) directory.recentChanged();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
}
