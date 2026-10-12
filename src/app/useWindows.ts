import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import type { SessionTarget } from "../lib/api";
import { tabTitle, type Pane, type Tab } from "../lib/panes";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { useTabStore, type TabStore, type TabTransfer } from "../lib/tabs";
import { useWindowEvent, windows, type WindowInfo } from "../lib/windows";

interface Options {
  followRemoteTitle: boolean;
  /** Opens a tab for a saved session (a window opened for one). */
  openTab(target: SessionTarget, title: string): void;
}

/**
 * This window among the others: its title, the other windows (where tabs can move to), what
 * it was opened to show, tabs moving in and out, and closing it once its last tab is gone
 * while other windows are open.
 *
 * A tab moves out as a `TabTransfer`, and stays here until each of its panes' sessions has
 * moved (`movedAway`, which the session registry calls): its terminals still show their
 * sessions meanwhile, and hand their screens over as the sessions move.
 */
export function useWindows(store: TabStore, sessions: SessionRegistry, { followRemoteTitle, openTab }: Options) {
  const { t } = useTranslation();
  const { tabs, activeKey } = useTabStore(store);
  const [others, setOthers] = useState<WindowInfo[]>([]);
  /** Tabs moving out, by key, with the panes whose sessions haven't moved yet. */
  const moving = useRef(new Map<number, Set<number>>());
  const openTabRef = useRef(openTab);
  openTabRef.current = openTab;

  const activeTab = tabs.find((tab) => tab.key === activeKey);
  const title = activeTab ? tabTitle(activeTab, followRemoteTitle) : t("app.name");
  useEffect(() => void windows.setTitle(title).catch(console.error), [title]);

  const refreshOthers = useCallback(() => void windows.list().then(setOthers, console.error), []);
  useEffect(refreshOthers, [refreshOthers]);
  useWindowEvent("windows-changed", refreshOthers);

  // Taken once: StrictMode's second run gets nothing.
  useEffect(() => {
    void windows.handoff().then((content) => {
      if (content?.kind === "tab") store.dispatch({ type: "insert", tab: content.tab });
      else if (content?.kind === "profile") openTabRef.current({ kind: "profile", profileId: content.profileId }, content.title);
    }, console.error);
  }, [store]);
  useWindowEvent<TabTransfer>("tab-received", (tab) => store.dispatch({ type: "insert", tab }));

  // Once the last tab has closed or moved away, the window goes too, unless it is the last
  // one, which stays empty as before.
  const hadTabs = useRef(false);
  useEffect(() => {
    if (tabs.length > 0) {
      hadTabs.current = true;
      return;
    }
    if (!hadTabs.current) return;
    hadTabs.current = false;
    void windows.list().then((list) => {
      if (list.length > 0) void getCurrentWindow().destroy();
    }, console.error);
  }, [tabs.length]);

  /**
   * Whether the tab can move now: not while its file panel transfers files, edits them in a
   * local editor or asks something (those belong to this window's page), nor during a ZMODEM
   * transfer, nor while a pane opens its session.
   */
  const canMove = useCallback(
    (tab: Tab) =>
      !moving.current.has(tab.key) &&
      tab.panes.every(
        (pane) =>
          pane.transfers === 0 &&
          pane.edits === 0 &&
          pane.questions === 0 &&
          !sessions.inZmodem(pane.key) &&
          !(pane.status === "connecting" && pane.sessionId === null),
      ),
    [sessions],
  );

  const finish = useCallback(
    (key: number) => {
      moving.current.delete(key);
      const tab = store.get().tabs.find((t) => t.key === key);
      if (tab) store.dispatch({ type: "close", keys: tab.panes.map((pane) => pane.key) });
    },
    [store],
  );

  /** Moves a tab to window `target`, or to a new window (null). */
  const moveTab = useCallback(
    async (key: number, target: string | null) => {
      const tab = store.get().tabs.find((t) => t.key === key);
      if (!tab || !canMove(tab)) return;
      const live = tab.panes.filter((pane) => pane.sessionId !== null).map((pane) => pane.key);
      const panes = tab.panes.map(
        (pane): Pane => ({
          ...pane,
          // Its first shell has long run on that connection; reconnecting makes its own.
          shareFrom: undefined,
          retryAt: null,
          arrival: {
            sessionId: pane.sessionId,
            connected: pane.status === "connected",
            snapshot: pane.sessionId === null ? (sessions.snapshot(pane.key) ?? null) : null,
            reconnect: pane.retryAt !== null,
          },
        }),
      );
      const transfer: TabTransfer = {
        layout: tab.layout,
        focused: tab.focused,
        customTitle: tab.customTitle,
        sidePanel: tab.sidePanel,
        panes,
      };
      moving.current.set(key, new Set(live));
      try {
        if (target === null) await windows.open({ kind: "tab", tab: transfer });
        else await windows.sendTab(target, transfer);
      } catch (e) {
        moving.current.delete(key);
        console.error(e);
        return;
      }
      if (live.length === 0) finish(key);
    },
    [store, sessions, canMove, finish],
  );

  /** The pane's session has moved to the other window (see `SessionRegistry`). */
  const movedAway = useCallback(
    (paneKey: number) => {
      for (const [key, waiting] of moving.current) {
        if (waiting.delete(paneKey) && waiting.size === 0) finish(key);
      }
    },
    [finish],
  );

  const openInNewWindow = useCallback(
    (profileId: string, title: string) => void windows.open({ kind: "profile", profileId, title }).catch(console.error),
    [],
  );

  return { others, canMove, moveTab, movedAway, openInNewWindow };
}
