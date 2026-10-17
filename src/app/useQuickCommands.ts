import { useEffect, useMemo, useState } from "react";

import { quickCommands, type QuickCommands } from "../ipc/api";
import { latestSave } from "../lib/latestSave";
import { storeBarVisible, storedBarVisible } from "../lib/quickCommands";
import { useConfigChanged } from "../hooks/windowEvents";

/** The quick commands, their bar under the terminals and their palette. */
export function useQuickCommands() {
  const [commands, setCommands] = useState<QuickCommands | null>(null);
  const [barOpen, setBarOpen] = useState(storedBarVisible);
  const [paletteOpen, setPaletteOpen] = useState(false);
  /** Why the last change couldn't be saved (it was undone), until dismissed. */
  const [saveError, setSaveError] = useState<unknown>(null);
  // Applied immediately; the stored copy (with ids for new commands) replaces it once saved
  // (see `latestSave`).
  const saver = useMemo(
    () =>
      latestSave<QuickCommands>({
        show: setCommands,
        store: quickCommands.set,
        load: quickCommands.get,
        failed: (e) => setSaveError(() => e),
      }),
    [],
  );

  useEffect(() => {
    quickCommands.get().then(saver.loaded).catch(console.error);
  }, [saver]);
  // Changed in another window.
  useConfigChanged("quickCommands", saver.reload);

  const toggleBar = () => {
    storeBarVisible(!barOpen);
    setBarOpen(!barOpen);
  };

  return {
    commands,
    save: saver.save,
    saveError,
    dismissSaveError: () => setSaveError(null),
    barOpen,
    toggleBar,
    paletteOpen,
    setPaletteOpen,
  };
}
