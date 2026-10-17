import { useEffect, useState } from "react";

import { quickCommands } from "../ipc/api";
import { quickCommandStore, storeBarVisible, storedBarVisible } from "../lib/quickCommands";
import { useConfigChanged } from "../hooks/windowEvents";

/**
 * The quick commands' bar under the terminals and their palette. Also keeps the quick commands
 * (`quickCommandStore`) up to date while the app is shown: read when it mounts, which is once
 * the vault is unlocked, again when another window changes them, and forgotten when it unmounts.
 */
export function useQuickCommandBar() {
  const [barOpen, setBarOpen] = useState(storedBarVisible);
  const [paletteOpen, setPaletteOpen] = useState(false);

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

  const toggleBar = () => {
    storeBarVisible(!barOpen);
    setBarOpen(!barOpen);
  };

  return { barOpen, toggleBar, paletteOpen, setPaletteOpen };
}
