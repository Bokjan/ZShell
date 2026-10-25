import { useEffect, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { sendBreak, sessionLog, type SessionTarget } from "../ipc/api";
import { announce } from "../lib/announce";
import { autoSide, canSplit, fits, insertLayout, paneKeys, removeFromLayout, type Direction, type Size } from "../lib/layout";
import { directory } from "../lib/directory";
import { findPane, focusedPane, targetProtocol, type Layout, type Pane } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { PanePlace, PaneSide, PaneSpec, TabStore } from "../lib/tabs";

/** How long a pane that can't be split flashes. */
const REFUSED_MS = 400;

/**
 * Opening tabs, splitting panes, copying them into new tabs, moving them between tabs, and
 * what a pane's menus do to its session (logging it, sending a break).
 */
export function usePaneActions(store: TabStore, sessions: SessionRegistry) {
  const { t } = useTranslation();
  // The pane that couldn't be split, flashing; `count` restarts the flash on each refusal.
  const [refused, setRefused] = useState<{ key: number; count: number } | null>(null);
  const refuseTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(refuseTimer.current), []);
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

  /** What a pane's next session will be: a saved session's protocol may have been changed. */
  const protocolOf = (target: SessionTarget) => targetProtocol(target, directory.get().profiles);

  /** A new pane for `target`, titled `title` until its program sets one. */
  const specOf = (target: SessionTarget, title: string): PaneSpec => ({ target, protocol: protocolOf(target), title });

  /** Opens a tab for `target` (see `specOf`). */
  const openTab = (target: SessionTarget, title: string) => store.dispatch({ type: "open", pane: specOf(target, title) });

  /**
   * A new pane running what `pane` runs: an SSH pane whose shell is up opens its copy on the
   * same connection; anything else opens the same target anew. So does one whose shell has
   * exited and that waits for forwarded connections: its port forwarding has stopped or moved
   * to another tab, and would not start again on the connection it shared.
   */
  const copyOf = (pane: Pane): PaneSpec => {
    const shares = pane.protocol === "ssh" && pane.status === "connected" && pane.waiting === null;
    const shareFrom = shares ? (pane.sessionId ?? undefined) : undefined;
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

  /** Starts or stops logging a pane's session. */
  const setLogging = (key: number, start: boolean) => {
    const pane = findPane(store.get().tabs, key)?.pane;
    if (pane?.sessionId == null) return;
    // The session reports the path (or an error) with a `log` event.
    store.dispatch({ type: "updatePane", key, patch: { logStopped: !start } });
    (start ? sessionLog.start(pane.sessionId) : sessionLog.stop(pane.sessionId)).catch(console.error);
  };

  /** Shows a pane's log file in the file manager. */
  const showLog = (key: number) => {
    const path = findPane(store.get().tabs, key)?.pane.logPath;
    if (path) revealItemInDir(path).catch(console.error);
  };

  /** Sends a break to a pane's session (serial and Telnet). */
  const breakSession = (key: number) => {
    const id = findPane(store.get().tabs, key)?.pane.sessionId;
    if (id != null) sendBreak(id).catch(console.error);
  };

  return {
    refused,
    onAreaSize,
    roomToSplit,
    specOf,
    openTab,
    splitPane,
    duplicateTab,
    placeIn,
    fitsBeside,
    movePane,
    mergeTab,
    setLogging,
    showLog,
    breakSession,
  };
}
