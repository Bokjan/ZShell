import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { useAppLock } from "../components/AppLockGate";
import type { SessionTarget } from "../lib/api";
import { paneWork, tabTitle, type Arrival, type Pane, type Tab } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { Dropped, Point } from "../lib/bindings";
import { tabIndexAt, tabStrip } from "../lib/tabStrip";
import { useTabStore, type TabStore, type TabTransfer } from "../lib/tabs";
import { useWindowEvent, windows, type DropTarget, type WindowInfo } from "../lib/windows";

interface Options {
  followRemoteTitle: boolean;
  /** Opens a tab for a saved session (a window opened for one). */
  openTab(target: SessionTarget, title: string): void;
}

/** A tab moving out of this window. */
interface Moving {
  /** The window it moves to, once known. */
  target: string | null;
  /** Its panes whose sessions haven't moved yet. */
  waiting: Set<number>;
  /** Its panes whose sessions couldn't move, which stay here. */
  stayed: Set<number>;
}

/**
 * This window among the others: its title, the other windows (where tabs can move to), what
 * it is to show (what it opened for, tabs given to it), tabs moving in and out, and closing it
 * once its last tab is gone while other windows are open.
 *
 * A tab moves out as a `TabTransfer`, and stays here until each of its panes' sessions has
 * moved (`movedAway`, which the session registry calls): its terminals still show their
 * sessions meanwhile, and hand their screens over as the sessions move. A pane whose session
 * couldn't move (the other window says so, or closes first) stays here, and the rest of the
 * tab goes.
 */
export function useWindows(store: TabStore, sessions: SessionRegistry, { followRemoteTitle, openTab }: Options) {
  const { t } = useTranslation();
  const { tabs, activeKey } = useTabStore(store);
  const [others, setOthers] = useState<WindowInfo[]>([]);
  /** Tabs moving out, by key. */
  const moving = useRef(new Map<number, Moving>());
  const openTabRef = useRef(openTab);
  openTabRef.current = openTab;

  // While the app lock covers the window, the title shows no session either: it is in the
  // Dock, the taskbar and the window switchers.
  const locked = useAppLock().status.locked;
  const activeTab = tabs.find((tab) => tab.key === activeKey);
  const title = activeTab && !locked ? tabTitle(activeTab, followRemoteTitle) : t("app.name");
  useEffect(() => void windows.setTitle(title).catch(console.error), [title]);

  const refreshOthers = useCallback(() => void windows.list().then(setOthers, console.error), []);
  useEffect(refreshOthers, [refreshOthers]);
  useWindowEvent("windows-changed", refreshOthers);

  // Once the last tab has closed or moved away, the window goes too, unless it is the last
  // one, which stays empty as before, or a tab is on its way here: not while taking tabs,
  // nor if one is waiting to be taken (which the backend knows).
  const hadTabs = useRef(false);
  const taking = useRef(0);
  const closeIfEmpty = useCallback(() => {
    if (!hadTabs.current || taking.current > 0 || store.get().tabs.length > 0) return;
    void windows.closeEmpty().catch(console.error);
  }, [store]);
  useEffect(() => {
    if (tabs.length > 0) hadTabs.current = true;
    else closeIfEmpty();
  }, [tabs.length, closeIfEmpty]);

  // Each is taken once: StrictMode's second run gets nothing.
  const take = useCallback(() => {
    taking.current++;
    void windows
      .take()
      .then((contents) => {
        for (const content of contents) {
          if (content.kind === "tab") {
            const strip = tabStrip();
            const index = content.at != null && strip ? tabIndexAt(strip, content.at) : undefined;
            store.dispatch({ type: "insert", tab: content.tab, index });
          } else openTabRef.current({ kind: "profile", profileId: content.profileId }, content.title);
        }
      }, console.error)
      .finally(() => {
        taking.current--;
        closeIfEmpty();
      });
  }, [store, closeIfEmpty]);
  useEffect(take, [take]);
  useWindowEvent("tab-received", take);

  /**
   * Whether the tab can move now: not while its file panel transfers files, edits them in a
   * local editor or asks something (those belong to this window's page), nor during a ZMODEM
   * transfer, nor while a pane opens its session or its session is still moving here.
   */
  const canMove = useCallback(
    (tab: Tab) =>
      !moving.current.has(tab.key) &&
      tab.panes.every(
        (pane) =>
          paneWork(pane) === null &&
          !sessions.inZmodem(pane.key) &&
          !(pane.status === "connecting" && pane.sessionId === null) &&
          pane.arrival === undefined &&
          !sessions.arriving(pane.key),
      ),
    [sessions],
  );

  /** Every pane of a moving tab has moved or stayed: the tab goes, but for those that stayed. */
  const settle = useCallback(
    (key: number) => {
      const move = moving.current.get(key);
      if (!move) return;
      moving.current.delete(key);
      const tab = store.get().tabs.find((t) => t.key === key);
      const gone = tab?.panes.filter((pane) => !move.stayed.has(pane.key)).map((pane) => pane.key) ?? [];
      if (gone.length > 0) store.dispatch({ type: "close", keys: gone });
    },
    [store],
  );

  /** Pane `paneKey` of a moving tab has moved (or stayed) by now. */
  const resolve = useCallback(
    (paneKey: number, stayed: boolean) => {
      for (const [key, move] of moving.current) {
        if (!move.waiting.delete(paneKey)) continue;
        if (stayed) move.stayed.add(paneKey);
        if (move.waiting.size === 0) settle(key);
      }
    },
    [settle],
  );

  /**
   * Moves a tab to window `target`, or to a new window (null). Dragged there: dropped `at`
   * that many CSS pixels from the left of the target's page, or `dropped` outside the windows,
   * where the new one opens.
   */
  const moveTab = useCallback(
    async (key: number, target: string | null, { at, dropped }: { at?: number; dropped?: Dropped } = {}) => {
      const tab = store.get().tabs.find((t) => t.key === key);
      if (!tab || !canMove(tab)) return;
      const here = getCurrentWindow().label;
      const live = tab.panes.filter((pane) => pane.sessionId !== null).map((pane) => pane.key);
      const panes = tab.panes.map((pane): Pane => ({
        ...pane,
        // The other window's once it has taken it over (see `arrival`): its panels don't use
        // it before, which the backend would refuse.
        sessionId: null,
        // Its first shell has long run on that connection; reconnecting makes its own.
        shareFrom: undefined,
        retryAt: null,
        arrival: {
          sessionId: pane.sessionId,
          connected: pane.status === "connected",
          snapshot: pane.sessionId === null ? (sessions.snapshot(pane.key) ?? null) : null,
          reconnect: pane.retryAt !== null,
          from: { window: here, pane: pane.key },
        },
      }));
      const transfer: TabTransfer = {
        layout: tab.layout,
        focused: tab.focused,
        customTitle: tab.customTitle,
        sidePanel: tab.sidePanel,
        panes,
      };
      const move: Moving = { target, waiting: new Set(live), stayed: new Set() };
      moving.current.set(key, move);
      // The other window reconnects those that were waiting to; one connection is enough.
      const closed = tab.panes.filter((pane) => pane.sessionId === null).map((pane) => pane.key);
      closed.forEach((paneKey) => sessions.suspend(paneKey));
      try {
        if (target === null) move.target = await windows.open({ kind: "tab", tab: transfer }, dropped);
        else await windows.sendTab(target, transfer, at);
      } catch (e) {
        moving.current.delete(key);
        closed.forEach((paneKey) => sessions.resume(paneKey));
        console.error(e);
        return;
      }
      if (live.length === 0) settle(key);
    },
    [store, sessions, canMove, settle],
  );

  /**
   * A tab dragged out of the window and dropped on `target`: into another window's tab bar, or
   * anywhere else into a window of its own, unless it is this window's only tab.
   */
  const dropTab = useCallback(
    (key: number, target: DropTarget, grab: Point) => {
      if (target.kind === "window") void moveTab(key, target.label, { at: target.x });
      else if (target.kind === "elsewhere" && store.get().tabs.length > 1)
        void moveTab(key, null, { dropped: { cursor: target.cursor, grab } });
    },
    [store, moveTab],
  );

  /** The pane's session has moved to the other window (see `SessionRegistry`). */
  const movedAway = useCallback((paneKey: number) => resolve(paneKey, false), [resolve]);
  // The other window couldn't take the pane's session, which is still here.
  useWindowEvent<number>("pane-stayed", (paneKey) => resolve(paneKey, true));
  // The window a tab was moving to closed before taking all its sessions: they are still here.
  useWindowEvent<string>("window-closed", (label) => {
    for (const [key, move] of moving.current) {
      if (move.target !== label) continue;
      move.waiting.forEach((paneKey) => move.stayed.add(paneKey));
      move.waiting.clear();
      settle(key);
    }
  });

  /**
   * The session of pane `key`, which came from another window, couldn't move here (it ended,
   * or a ZMODEM transfer started there): that window keeps it, and the pane goes.
   */
  const stayed = useCallback(
    (key: number, from: Arrival["from"]) => {
      void windows.paneStayed(from.window, from.pane).catch(console.error);
      store.dispatch({ type: "close", keys: [key] });
    },
    [store],
  );

  const openInNewWindow = useCallback(
    (profileId: string, title: string) => void windows.open({ kind: "profile", profileId, title }).catch(console.error),
    [],
  );

  return { others, canMove, moveTab, dropTab, movedAway, stayed, openInNewWindow };
}
