import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { useAppLock } from "../hooks/appLock";
import type { SessionTarget } from "../ipc/api";
import { paneWork, tabTitle, type Arrival, type Pane, type Tab } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { Dropped, Point } from "../ipc/bindings";
import { tabIndexAt, tabStrip } from "../lib/tabStrip";
import { TabMoves, type Settled } from "../lib/tabMoves";
import { leavesSomething, type TabStore, type TabTransfer } from "../lib/tabs";
import { useTabStore } from "../hooks/stores";
import { windows, type DropTarget, type WindowInfo } from "../ipc/windows";
import { useWindowEvent } from "../hooks/windowEvents";
import { useLatest } from "../hooks/latest";

interface Options {
  followRemoteTitle: boolean;
  /** Opens a tab for a saved session (a window opened for one). */
  openTab(target: SessionTarget, title: string): void;
}

/**
 * A window's title goes to the system at most this often while it keeps changing: each change
 * is told to every window, which asks for the list of windows again.
 */
const TITLE_MS = 500;

/** The panes of the tab that move: all of them, or only pane `only`. */
const panesMoving = (tab: Tab, only?: number) => (only === undefined ? tab.panes : tab.panes.filter((pane) => pane.key === only));

const sameWindows = (a: WindowInfo[], b: WindowInfo[]) =>
  a.length === b.length && a.every((window, i) => window.label === b[i].label && window.title === b[i].title);

/**
 * This window among the others: its title, the other windows (where tabs can move to), what
 * it is to show (what it opened for, tabs given to it), tabs moving in and out, and closing it
 * once its last tab is gone while other windows are open.
 *
 * A tab moves out as a `TabTransfer`, and stays here until each of its panes' sessions has
 * moved (`movedAway`, heard from the session registry): its terminals still show their
 * sessions meanwhile, and hand their screens over as the sessions move. A pane whose session
 * couldn't move (the other window says so, or closes first) stays here, and the rest of the
 * tab goes.
 */
export function useWindows(store: TabStore, sessions: SessionRegistry, { followRemoteTitle, openTab }: Options) {
  const { t } = useTranslation();
  const { tabs, activeKey } = useTabStore(store);
  const [others, setOthers] = useState<WindowInfo[]>([]);
  /** Tabs moving out. */
  const moves = useRef(new TabMoves());
  const openTabRef = useLatest(openTab);

  // While the app lock covers the window, the title shows no session either: it is in the
  // Dock, the taskbar and the window switchers.
  const locked = useAppLock().status.locked;
  const activeTab = tabs.find((tab) => tab.key === activeKey);
  const title = activeTab && !locked ? tabTitle(activeTab, followRemoteTitle) : t("app.name");
  // A title that keeps changing (a program's spinner) at most every `TITLE_MS`, the last one
  // always; the title of a locked window at once.
  const titleSetAt = useRef(-Infinity);
  useEffect(() => {
    const set = () => {
      titleSetAt.current = Date.now();
      void windows.setTitle(title).catch(console.error);
    };
    const wait = locked ? 0 : titleSetAt.current + TITLE_MS - Date.now();
    if (wait <= 0) {
      set();
      return;
    }
    const timer = setTimeout(set, wait);
    return () => clearTimeout(timer);
  }, [title, locked]);

  // Every window hears of every change of a window's title: only a change to the list
  // renders this one again.
  const refreshOthers = useCallback(
    () => void windows.list().then((list) => setOthers((others) => (sameWindows(others, list) ? others : list)), console.error),
    [],
  );
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
  }, [store, closeIfEmpty, openTabRef]);
  useEffect(take, [take]);
  useWindowEvent("tab-received", take);

  /**
   * Whether the tab (or only its pane `pane`) can move now: not while its file panel transfers
   * files, edits them in a local editor or asks something (those belong to this window's page),
   * nor during a ZMODEM transfer, nor while a pane opens its session or its session is still
   * moving here.
   */
  const canMove = useCallback(
    (tab: Tab, only?: number) =>
      !tab.moving &&
      panesMoving(tab, only).every(
        (pane) =>
          paneWork(pane) === null &&
          !sessions.inZmodem(pane.key) &&
          !(pane.status === "connecting" && pane.sessionId === null) &&
          pane.arrival === undefined &&
          !sessions.arriving(pane.key),
      ),
    [sessions],
  );

  /** Moves that are over: the panes that moved go, but for those that stayed. */
  const settle = useCallback(
    (settled: Settled[]) => {
      for (const { key, gone } of settled) {
        if (!store.get().tabs.some((t) => t.key === key)) continue;
        store.dispatch({ type: "updateTab", key, patch: { moving: false } });
        if (gone.length > 0) store.dispatch({ type: "close", keys: gone });
      }
    },
    [store],
  );

  /**
   * Moves a tab to window `target`, or to a new window (null); with `pane`, only that pane,
   * which goes there as a tab of its own and leaves the rest of its tab here. Dragged there:
   * dropped `at` that many CSS pixels from the left of the target's page, or `dropped` outside
   * the windows, where the new one opens.
   */
  const moveTab = useCallback(
    async (key: number, target: string | null, { at, dropped, pane: only }: { at?: number; dropped?: Dropped; pane?: number } = {}) => {
      const tab = store.get().tabs.find((t) => t.key === key);
      if (!tab || !canMove(tab, only)) return;
      const here = getCurrentWindow().label;
      const moving = panesMoving(tab, only);
      // The pane closed meanwhile (a drag dropped after its shell exited).
      if (moving.length === 0) return;
      const live = moving.filter((pane) => pane.sessionId !== null).map((pane) => pane.key);
      const panes = moving.map((pane): Pane => ({
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
      const transfer: TabTransfer =
        only === undefined || tab.panes.length === 1
          ? { layout: tab.layout, focused: tab.focused, customTitle: tab.customTitle, sidePanel: tab.sidePanel, panes }
          : {
              layout: { kind: "pane", key: only },
              focused: only,
              customTitle: null,
              sidePanel: tab.focused === only ? tab.sidePanel : null,
              panes,
            };
      moves.current.start(
        key,
        target,
        moving.map((pane) => pane.key),
        live,
      );
      store.dispatch({ type: "updateTab", key, patch: { moving: true } });
      // The other window reconnects those that were waiting to; one connection is enough.
      const closed = moving.filter((pane) => pane.sessionId === null).map((pane) => pane.key);
      closed.forEach((paneKey) => sessions.suspend(paneKey));
      try {
        if (target === null) settle(moves.current.opened(key, await windows.open({ kind: "tab", tab: transfer, at: null }, dropped)));
        else await windows.sendTab(target, transfer, at);
      } catch (e) {
        moves.current.cancel(key);
        store.dispatch({ type: "updateTab", key, patch: { moving: false } });
        closed.forEach((paneKey) => sessions.resume(paneKey));
        console.error(e);
        return;
      }
      settle(moves.current.started(key));
    },
    [store, sessions, canMove, settle],
  );

  /**
   * A tab (or its pane `pane`) dragged out of the window and dropped on `target`: into another
   * window's tab bar, or anywhere else into a window of its own, unless it is this window's
   * only tab.
   */
  const dropTab = useCallback(
    (key: number, target: DropTarget, grab: Point, pane?: number) => {
      if (target.kind === "window") void moveTab(key, target.label, { at: target.x, pane });
      else if (target.kind === "elsewhere" && leavesSomething(store.get().tabs, key, pane))
        void moveTab(key, null, { dropped: { cursor: target.cursor, grab }, pane });
    },
    [store, moveTab],
  );

  /** The pane's session has moved to the other window (see `SessionRegistry`). */
  const movedAway = useCallback((paneKey: number) => settle(moves.current.resolve(paneKey, false)), [settle]);
  // The other window couldn't take the pane's session, which is still here.
  useWindowEvent("pane-stayed", (paneKey) => settle(moves.current.resolve(paneKey, true)));
  // The window a tab was moving to closed before taking all its sessions: they are still here.
  useWindowEvent("window-closed", (label) => settle(moves.current.windowClosed(label)));
  // The page of the window a tab was moving to loaded again before taking these sessions,
  // which are still here.
  useWindowEvent("offers-withdrawn", (ids) => {
    const panes = store
      .get()
      .tabs.filter((tab) => tab.moving)
      .flatMap((tab) => tab.panes)
      .filter((pane) => pane.sessionId !== null && ids.includes(pane.sessionId));
    settle(panes.flatMap((pane) => moves.current.resolve(pane.key, true)));
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

  useEffect(() => sessions.on("movedAway", movedAway), [sessions, movedAway]);
  useEffect(() => sessions.on("stayed", stayed), [sessions, stayed]);

  return { others, canMove, moveTab, dropTab, openInNewWindow };
}
