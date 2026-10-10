import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { forwards, sessionForeground, type ForwardRule, type SessionId, type Settings } from "../lib/api";
import { isDialogOpen } from "../lib/dialogs";
import { forwardMapping } from "../lib/format";
import { findPane, paneLabel, tabTitle, type Pane } from "../lib/panes";
import { activeTabOf, type TabStore } from "../lib/tabs";

/** Why closing a pane needs confirmation: a remote session is connected, or a local program runs. */
type Busy = { kind: "connected" } | { kind: "process"; name: string } | { kind: "transfers"; count: number };

interface Closing {
  panes: number[];
  /** How many tabs are being closed; 0 for a pane. */
  tabs: number;
  busy: Busy;
  /** Of the busy pane. */
  name: string;
}

export type CloseRequest = { kind: "tabs"; keys: number[] } | { kind: "pane"; key: number };

async function busyReason(pane: Pane): Promise<Busy | null> {
  if (pane.transfers > 0) return { kind: "transfers", count: pane.transfers };
  if (pane.status !== "connected" || pane.sessionId == null) return null;
  if (pane.target.kind !== "local") return { kind: "connected" };
  const name = await sessionForeground(pane.sessionId).catch(() => null);
  return name === null ? null : { kind: "process", name };
}

/**
 * Closing tabs, panes and the window, asking first when a pane is busy (see the settings), and
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
  // The window waiting for the user to confirm closing it (quitting), with how many tabs are busy.
  const [closingWindow, setClosingWindow] = useState<number | null>(null);

  const closePanes = useCallback((keys: number[]) => store.dispatch({ type: "close", keys }), [store]);

  /** Closes tabs or a pane, first asking if any pane is connected or running a program. */
  const requestClose = useCallback(
    async (request: CloseRequest) => {
      const panes =
        request.kind === "tabs"
          ? store.get().tabs.filter((t) => request.keys.includes(t.key)).flatMap((tab) => tab.panes.map((pane) => ({ tab, pane })))
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

  /** ⌘W / Ctrl+Shift+W: the focused pane of a split tab, otherwise the tab (the window if there is none). */
  const closeFocused = useCallback(() => {
    const tab = activeTabOf(store.get());
    if (!tab) void getCurrentWindow().close();
    else if (tab.panes.length > 1) void requestClose({ kind: "pane", key: tab.focused });
    else void requestClose({ kind: "tabs", keys: [tab.key] });
  }, [store, requestClose]);

  // Closing the window (the close button, ⌘Q, Alt+F4) quits, ending every session at once,
  // so it always asks, whatever the setting for closing tabs.
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      const reasons = await Promise.all(store.get().tabs.flatMap((tab) => tab.panes).map(busyReason));
      const busy = reasons.filter((reason) => reason !== null).length;
      if (busy === 0) return;
      event.preventDefault();
      setClosingWindow(busy);
    });
    return () => void unlisten.then((f) => f());
  }, [store]);

  // From the macOS File menu's "Close" item (⌘W), which closes the window once no tabs are
  // left, as in Terminal.app.
  useEffect(() => {
    const unlisten = listen("close-tab", () => !isDialogOpen() && closeFocused());
    return () => void unlisten.then((f) => f());
  }, [closeFocused]);

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
    return busy.name ? t(`closeConfirm.process.${what}`, { process: busy.name, name }) : t(`closeConfirm.processUnknown.${what}`, { name });
  };

  const dialogs = (
    <>
      {closingWindow !== null && (
        <ConfirmDialog
          title={t("closeConfirm.windowTitle")}
          message={t("closeConfirm.window", { count: closingWindow })}
          confirmLabel={t("closeConfirm.windowConfirm")}
          danger
          onConfirm={() => void getCurrentWindow().destroy()}
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

  return { requestClose, closeFocused, dialogs };
}
