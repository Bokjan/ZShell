import { useEffect, useRef, useState } from "react";

import { quickCommands, type QuickCommands } from "../lib/api";
import { storeBarVisible, storedBarVisible } from "../lib/quickCommands";

/** The quick commands, their bar under the terminals and their palette. */
export function useQuickCommands() {
  const [commands, setCommands] = useState<QuickCommands | null>(null);
  const [barOpen, setBarOpen] = useState(storedBarVisible);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const latest = useRef(0);

  useEffect(() => {
    quickCommands.get().then(setCommands).catch(console.error);
  }, []);

  // Applied immediately; the stored copy (with ids for new commands) replaces it unless a
  // newer change was made in the meantime.
  const save = (next: QuickCommands) => {
    const request = ++latest.current;
    setCommands(next);
    quickCommands.set(next).then((saved) => request === latest.current && setCommands(saved), console.error);
  };

  const toggleBar = () => {
    storeBarVisible(!barOpen);
    setBarOpen(!barOpen);
  };

  return { commands, save, barOpen, toggleBar, paletteOpen, setPaletteOpen };
}
