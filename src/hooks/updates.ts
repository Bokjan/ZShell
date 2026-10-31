import { useEffect, useSyncExternalStore } from "react";

import { followUpdates, updateState } from "../lib/updates";
import { useSettings } from "./settings";

/**
 * The state of the update check, and `available`: the newer release unless the user skipped
 * it, for which the session list shows a button. About shows a skipped one too (`newer`).
 */
export function useUpdates() {
  useEffect(followUpdates, []);
  const current = useSyncExternalStore(updateState.subscribe, updateState.get);
  const { settings } = useSettings();
  const available = current.newer && current.newer.version !== settings.updates.skipped ? current.newer : null;
  return { ...current, available };
}
