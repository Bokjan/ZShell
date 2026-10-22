import { useState } from "react";

import { storeBarVisible, storedBarVisible } from "../lib/quickCommands";

/** The quick commands' bar under the terminals and their palette (shown or not). */
export function useQuickCommandBar() {
  const [barOpen, setBarOpen] = useState(storedBarVisible);
  const [paletteOpen, setPaletteOpen] = useState(false);

  const toggleBar = () => {
    storeBarVisible(!barOpen);
    setBarOpen(!barOpen);
  };

  return { barOpen, toggleBar, paletteOpen, setPaletteOpen };
}
