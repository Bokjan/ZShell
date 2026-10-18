import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { SessionTarget } from "../ipc/api";
import { announce } from "../lib/announce";
import { autoSide, canSplit, fits, insertLayout, paneKeys, removeFromLayout, type Direction, type Size } from "../lib/layout";
import { findPane, focusedPane, type Layout, type Pane, type TabProtocol } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { PanePlace, PaneSide, PaneSpec, TabStore } from "../lib/tabs";

/** How long a pane that can't be split flashes. */
const REFUSED_MS = 400;

/** Splitting panes, copying them into new tabs, and moving them between tabs. */
export function usePaneActions(store: TabStore, sessions: SessionRegistry, protocolOf: (target: SessionTarget) => TabProtocol) {
  const { t } = useTranslation();
  // The pane that couldn't be split, flashing; `count` restarts the flash on each refusal.
  const [refused, setRefused] = useState<{ key: number; count: number } | null>(null);
  const refuseTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  // Each tab's pane area, as its page last measured it.
  const areas = useRef(new Map<number, Size>());

  const onAreaSize = (tabKey: number, size: Size) => {
    // A tab measured for the first time: those closed since go.
    if (!areas.current.has(tabKey)) {
      const open = new Set(store.get().tabs.map((tab) => tab.key));
      for (const key of areas.current.keys()) if (!open.has(key)) areas.current.delete(key);
    }
    areas.current.set(tabKey, size);
  };

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

  /**
   * Where a pane moved into tab `tabKey` from a menu goes: next to the tab's focused pane, on
   * the side that has room (see `autoSide`); null when neither has.
   */
  const placeIn = (tabKey: number): PaneSide | null => {
    const tab = store.get().tabs.find((t) => t.key === tabKey);
    if (!tab || tab.moving) return null;
    const side = autoSide(tab.layout, tab.focused, areas.current.get(tab.key));
    return side && { key: tab.focused, side };
  };

  /**
   * Whether panes arranged as `added` fit next to pane `to.key` on its side, the pane and each
   * of them at least the minimum size; `moving` is a pane that leaves its place first (in the
   * same tab, it frees its space).
   */
  const fitsBeside = (added: Layout, to: PaneSide, moving?: number) => {
    const found = findPane(store.get().tabs, to.key);
    if (!found) return false;
    const { layout } = found.tab;
    const left = moving !== undefined && found.tab.panes.some((pane) => pane.key === moving) ? removeFromLayout(layout, moving) : layout;
    if (!left) return false;
    return fits(insertLayout(left, to.key, to.side, added), [to.key, ...paneKeys(added)], areas.current.get(found.tab.key));
  };

  /** Moves a pane to a tab of its own or next to another pane; its terminal keeps the keyboard. */
  const movePane = (key: number, to: PanePlace) => {
    store.dispatch({ type: "movePane", key, to });
    requestAnimationFrame(() => sessions.focus(key));
  };

  /** Moves a tab's panes next to a pane of another tab, its focused pane keeping the keyboard. */
  const mergeTab = (key: number, to: PaneSide) => {
    const tab = store.get().tabs.find((t) => t.key === key);
    if (!tab) return;
    store.dispatch({ type: "mergeTab", key, to });
    requestAnimationFrame(() => sessions.focus(tab.focused));
  };

  return { refused, onAreaSize, roomToSplit, splitPane, duplicateTab, placeIn, fitsBeside, movePane, mergeTab };
}
