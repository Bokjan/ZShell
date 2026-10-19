import { useEffect } from "react";

import { windows } from "../ipc/windows";
import { mergeProgress, type Tab } from "../lib/panes";

/**
 * The progress programs report in the window's tabs (OSC 9;4), merged, on its taskbar button;
 * on the Dock icon and the launcher, which the app has one of, the backend merges it with the
 * other windows' (`window/progress.rs`).
 */
export function useTaskbarProgress(tabs: Tab[]) {
  const progress = mergeProgress(tabs.flatMap((tab) => tab.panes.map((pane) => pane.progress)));
  const state = progress?.state ?? null;
  const percent = progress?.percent ?? 0;
  useEffect(() => {
    windows.setProgress(state && { state, percent });
  }, [state, percent]);
}
