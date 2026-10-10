import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { SessionTarget } from "../lib/api";
import { announce } from "../lib/announce";
import { canSplit, type Direction, type Size } from "../lib/layout";
import { findPane, focusedPane, type Pane, type TabProtocol } from "../lib/panes";
import type { PaneSpec, TabStore } from "../lib/tabs";

/** How long a pane that can't be split flashes. */
const REFUSED_MS = 400;

/** Splitting panes and copying them into new tabs. */
export function usePaneActions(store: TabStore, protocolOf: (target: SessionTarget) => TabProtocol) {
  const { t } = useTranslation();
  // The pane that couldn't be split, flashing; `count` restarts the flash on each refusal.
  const [refused, setRefused] = useState<{ key: number; count: number } | null>(null);
  const refuseTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  // Each tab's pane area, as its page last measured it.
  const areas = useRef(new Map<number, Size>());

  const onAreaSize = (tabKey: number, size: Size) => areas.current.set(tabKey, size);

  /** Whether the pane is large enough to split in two, each half at least the minimum size. */
  const roomToSplit = (key: number, direction: Direction) => {
    const found = findPane(store.get().tabs, key);
    return !found || canSplit(found.tab.layout, key, areas.current.get(found.tab.key), direction);
  };

  /**
   * A new pane running what `pane` runs: an SSH pane whose shell is up opens its copy on the
   * same connection; anything else opens the same target anew.
   */
  const copyOf = (pane: Pane): PaneSpec => {
    const shareFrom = pane.protocol === "ssh" && pane.status === "connected" ? (pane.sessionId ?? undefined) : undefined;
    return { target: pane.target, protocol: protocolOf(pane.target), title: pane.title, shareFrom };
  };

  // Copies the focused pane into a new tab. A serial device can only be open once.
  const duplicateTab = (key: number) => {
    const tab = store.get().tabs.find((t) => t.key === key);
    if (!tab) return;
    const pane = focusedPane(tab);
    if (pane.protocol === "serial") return;
    store.dispatch({ type: "open", pane: copyOf(pane), after: key });
  };

  /**
   * Splits a pane, opening `added` (by default a copy of the pane) right of it (`row`) or
   * below it (`column`), and focuses it. Not for a pane too small to split, nor to copy a
   * serial pane (a device can only be open once).
   */
  const splitPane = (key: number, direction: Direction, added?: PaneSpec) => {
    const found = findPane(store.get().tabs, key);
    if (!found) return;
    // Shortcuts and the session list can ask for what the menus disable: the pane flashes.
    const serial = !added && found.pane.protocol === "serial";
    if (serial || !roomToSplit(key, direction)) {
      announce(t(serial ? "announce.splitSerial" : "announce.splitNoRoom"));
      clearTimeout(refuseTimer.current);
      setRefused((refused) => ({ key, count: (refused?.count ?? 0) + 1 }));
      refuseTimer.current = setTimeout(() => setRefused(null), REFUSED_MS);
      return;
    }
    store.dispatch({ type: "split", key, direction, pane: added ?? copyOf(found.pane) });
  };

  return { refused, onAreaSize, roomToSplit, splitPane, duplicateTab };
}
