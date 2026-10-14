import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { useAppLock, type PendingClose } from "../components/AppLockGate";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { forwards, sessionForeground, type ForwardRule, type SessionId, type Settings } from "../lib/api";
import { isAppLocked } from "../lib/appLock";
import { forwardMapping } from "../lib/format";
import { matches } from "../lib/keymap";
import { findPane, paneLabel, paneWork, tabTitle, type Pane, type PaneWork } from "../lib/panes";
import { useShortcuts } from "../lib/shortcuts";
import { activeTabOf, type TabStore } from "../lib/tabs";
import { useWindowEvent, windows } from "../lib/windows";

/**
 * Why closing a pane needs confirmation: its file panel is at work (even disconnected), a
 * remote session is connected, or a local program runs.
 */
type Busy = PaneWork | { kind: "connected" } | { kind: "process"; name: string };

interface Closing {
  panes: number[];
  /** How many tabs are being closed; 0 for a pane. */
  tabs: number;
  busy: Busy;
  /** Of the busy pane. */
  name: string;
}

export type CloseRequest = { kind: "tabs"; keys: number[] } | { kind: "pane"; key: number };

/** How many panes of the window closing it would end. */
async function busyPanes(store: TabStore): Promise<number> {
  const reasons = await Promise.all(
    store
      .get()
      .tabs.flatMap((tab) => tab.panes)
      .map(busyReason),
  );
  return reasons.filter((reason) => reason !== null).length;
}

async function busyReason(pane: Pane): Promise<Busy | null> {
  const work = paneWork(pane);
  if (work) return work;
  if (pane.status !== "connected" || pane.sessionId == null) return null;
  if (pane.target.kind !== "local") return { kind: "connected" };
  const name = await sessionForeground(pane.sessionId).catch(() => null);
  return name === null ? null : { kind: "process", name };
}

/**
 * Closing tabs, panes and the window, and quitting, asking first when a pane is busy (see the
 * settings for tabs and panes; closing a window or quitting always asks), and
 * whether port forwarding another tab of the session could take over should keep running.
 * `dialogs` is what to render for the questions.
 */
export function useCloseFlow(store: TabStore, settings: Settings, update: (settings: Settings) => void) {
  const { t } = useTranslation();
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  // Panes waiting for the user to confirm closing them, with why the first busy one is busy.
  const [closing, setClosing] = useState<Closing | null>(null);
  // Closing panes whose connections run port forwarding another tab of the session could take over.
  const [keeping, setKeeping] = useState<{ panes: number[]; ids: SessionId[]; rules: ForwardRule[] } | null>(null);
  // The window waiting for the user to confirm closing it, or quitting.
  const [closingWindow, setClosingWindow] = useState<PendingClose | null>(null);

  // While the app is locked the dialogs are under the lock screen, which asks instead; a close
  // still waiting there when it is unlocked is asked again here.
  const { confirmClose: confirmCloseLocked, pendingClose, clearPendingClose } = useAppLock();
  useEffect(() => {
    if (pendingClose === null) return;
    clearPendingClose();
    setClosingWindow(pendingClose);
  }, [pendingClose, clearPendingClose]);

  const closePanes = useCallback((keys: number[]) => store.dispatch({ type: "close", keys }), [store]);

  /** Closes tabs or a pane, first asking if any pane is connected or running a program. */
  const requestClose = useCallback(
    async (request: CloseRequest) => {
      const panes =
        request.kind === "tabs"
          ? store
              .get()
              .tabs.filter((t) => request.keys.includes(t.key))
              .flatMap((tab) => tab.panes.map((pane) => ({ tab, pane })))
          : [findPane(store.get().tabs, request.key)].filter((found) => found !== null);
      const keys = panes.map(({ pane }) => pane.key);
      // Asking whether to keep the forwarding also asks whether to close.
      const ids = panes.flatMap(({ pane }) => (pane.status === "connected" && pane.sessionId != null ? [pane.sessionId] : []));
      const rules = ids.length > 0 ? await forwards.keepCandidates(ids).catch(() => []) : [];
      if (rules.length > 0) {
        setKeeping({ panes: keys, ids, rules });
        return;
      }
      if (settingsRef.current.tabs.confirmClose) {
        const reasons = await Promise.all(panes.map(({ pane }) => busyReason(pane)));
        const index = reasons.findIndex((reason) => reason !== null);
        if (index >= 0) {
          const { tab, pane } = panes[index];
          const follow = settingsRef.current.tabs.followRemoteTitle;
          // A split tab is named as a whole (see `message`).
          const name = request.kind === "tabs" && tab.panes.length > 1 ? tabTitle(tab, follow) : paneLabel(tab, pane, follow);
          const tabs = request.kind === "tabs" ? request.keys.length : 0;
          setClosing({ panes: keys, tabs, busy: reasons[index]!, name });
          return;
        }
      }
      closePanes(keys);
    },
    [store, closePanes],
  );

  /** Closes a pane of a split tab, otherwise the tab. */
  const closePane = useCallback(
    (key: number) => {
      const found = findPane(store.get().tabs, key);
      if (!found) return;
      if (found.tab.panes.length > 1) void requestClose({ kind: "pane", key });
      else void requestClose({ kind: "tabs", keys: [found.tab.key] });
    },
    [store, requestClose],
  );

  /** ⌘W / Ctrl+Shift+W: the focused pane of a split tab, otherwise the tab (the window if there is none). */
  const closeFocused = useCallback(() => {
    const tab = activeTabOf(store.get());
    if (!tab) void getCurrentWindow().close();
    else closePane(tab.focused);
  }, [store, closePane]);

  // Closing the window (the close button, ⇧⌘W, Alt+F4) ends its sessions at once, so it
  // always asks, whatever the setting for closing tabs; the last window closing quits.
  // Quitting from the macOS Dock, logging out or shutting down ends the app without asking (a
  // known limitation).
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      const busy = await busyPanes(store);
      if (busy === 0) return;
      event.preventDefault();
      if (isAppLocked()) confirmCloseLocked({ busy, quit: false });
      else setClosingWindow({ busy, quit: false });
    });
    return () => void unlisten.then((f) => f());
  }, [store, confirmCloseLocked]);

  // Quitting (⌘Q, from the window that had the focus) asks once for all the windows: each
  // says how many of its panes are busy.
  useWindowEvent<number>("busy-query", (query) => void busyPanes(store).then((count) => windows.busyReply(query, count)));
  const quit = async () => {
    const busy = await windows.busyCount().catch(() => 0);
    if (busy === 0) void windows.quit();
    else if (isAppLocked()) confirmCloseLocked({ busy, quit: true });
    else setClosingWindow({ busy, quit: true });
  };
  useWindowEvent("quit-requested", () => void quit());

  // The app menu's items outside macOS, where the backend has them (menu.rs) once the page
  // has put them in the menu bar.
  useShortcuts(
    (e) => {
      if (matches(e, "closeWindow")) void getCurrentWindow().close();
      else if (matches(e, "quit")) void quit();
      else return false;
      return true;
    },
    { when: "always" },
  );

  const confirmClose = (dontAskAgain: boolean) => {
    if (!closing) return;
    if (dontAskAgain) update({ ...settings, tabs: { ...settings.tabs, confirmClose: false } });
    closePanes(closing.panes);
    setClosing(null);
  };
  const cancelClose = useCallback(() => setClosing(null), []);

  const closeKeeping = async (keep: boolean) => {
    if (!keeping) return;
    setKeeping(null);
    if (keep) await forwards.keep(keeping.ids).catch(console.error);
    closePanes(keeping.panes);
  };
  const cancelKeeping = useCallback(() => setKeeping(null), []);

  const cancelCloseWindow = useCallback(() => setClosingWindow(null), []);

  const message = ({ panes, tabs, busy, name }: Closing) => {
    if (panes.length > 1 && tabs > 1) return t("closeConfirm.many");
    // A tab's panes are named; a lone pane or tab ends just its session.
    if (panes.length > 1) return t("closeConfirm.panes", { name });
    const what = tabs === 0 ? "pane" : "tab";
    if (busy.kind === "connected") return t(`closeConfirm.connected.${what}`, { name });
    if (busy.kind === "transfers") return t(`closeConfirm.transfers.${what}`, { count: busy.count, name });
    if (busy.kind === "edits") return t(`closeConfirm.edits.${what}`, { count: busy.count, name });
    if (busy.kind === "questions") return t(`closeConfirm.questions.${what}`, { count: busy.count, name });
    return busy.name ? t(`closeConfirm.process.${what}`, { process: busy.name, name }) : t(`closeConfirm.processUnknown.${what}`, { name });
  };

  const dialogs = (
    <>
      {closingWindow !== null && (
        <ConfirmDialog
          title={closingWindow.quit ? t("closeConfirm.quitTitle", { name: t("app.name") }) : t("closeConfirm.windowTitle")}
          message={t(closingWindow.quit ? "closeConfirm.quit" : "closeConfirm.window", { count: closingWindow.busy })}
          confirmLabel={t(closingWindow.quit ? "closeConfirm.quitConfirm" : "closeConfirm.windowConfirm")}
          danger
          onConfirm={() => void (closingWindow.quit ? windows.quit() : getCurrentWindow().destroy())}
          onCancel={cancelCloseWindow}
        />
      )}
      {keeping && (
        <ConfirmDialog
          title={t("closeForwards.title")}
          message={t("closeForwards.message")}
          confirmLabel={t("closeForwards.keep")}
          secondaryLabel={t("closeForwards.stop")}
          onSecondary={() => void closeKeeping(false)}
          onConfirm={() => void closeKeeping(true)}
          onCancel={cancelKeeping}
        >
          <ul className="dialog-list">
            {keeping.rules.map((rule) => (
              <li key={rule.id}>{forwardMapping(rule)}</li>
            ))}
          </ul>
        </ConfirmDialog>
      )}
      {closing && (
        <ConfirmDialog
          title={
            closing.tabs === 0
              ? t("closeConfirm.paneTitle")
              : closing.tabs > 1
                ? t("closeConfirm.titleMany", { count: closing.tabs })
                : t("closeConfirm.title")
          }
          message={message(closing)}
          confirmLabel={t("closeConfirm.confirm")}
          danger
          checkboxLabel={t("common.dontAskAgain")}
          onConfirm={confirmClose}
          onCancel={cancelClose}
        />
      )}
    </>
  );

  return { requestClose, closePane, closeFocused, dialogs };
}
