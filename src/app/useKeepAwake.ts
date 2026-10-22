import { useEffect } from "react";

import { windows } from "../ipc/windows";
import { keepsAwake, type Tab } from "../lib/panes";

/**
 * Tells the backend whether any of the window's tabs is connected: it keeps the system awake
 * while one in any window is, as the settings say (`keep_awake/`).
 */
export function useKeepAwake(tabs: Tab[]) {
  const connected = tabs.some((tab) => tab.panes.some(keepsAwake));
  useEffect(() => windows.setConnected(connected), [connected]);
}
