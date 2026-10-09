import { Fragment, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";

import type { ForwardState, LogOpen, Profile, SessionId } from "../lib/api";
import type { MenuItem } from "./ContextMenu";
import { dragHorizontally } from "../lib/drag";
import { focusedPane, type Pane, type SidePanel, type Tab } from "../lib/panes";
import { ForwardsPanel } from "./ForwardsPanel";
import { SftpPanel } from "./SftpPanel";
import { TerminalView, type SessionStatus } from "./TerminalView";

/** What a tab page reports about its panes, and asks for them; each by pane key. */
export interface PaneHandlers {
  onStatus(key: number, status: SessionStatus): void;
  onExited(key: number, status: number | null): void;
  onSession(key: number, id: SessionId | null): void;
  onForward(key: number, ruleId: string, state: ForwardState): void;
  onTitle(key: number, title: string): void;
  /** What the user typed in the pane's terminal (see `TerminalView`). */
  onInput(key: number, data: string): void;
  onLog(key: number, path: string | null): void;
  /** The pane was clicked or got the keyboard focus. */
  onFocus(key: number): void;
  /** Added to the end of the pane's terminal menu. */
  menuItems(pane: Pane): MenuItem[];
  logOpen(pane: Pane): LogOpen;
  /** The pane's saved session; undefined for local terminals and deleted sessions. */
  profileOf(pane: Pane): Profile | undefined;
  onProfileChanged(profile: Profile): void;
}

interface Props {
  tab: Tab;
  active: boolean;
  /** The pane whose input is sent to other panes too (the compose bar's sync); marked. */
  syncing: number | null;
  handlers: PaneHandlers;
}

const MIN_PANEL = 280;
const MIN_TERMINAL = 240;

/**
 * One tab's content: its panes, each a terminal, plus a side panel (files or port forwards)
 * on the focused pane's SSH connection.
 */
export function TabPage({ tab, active, syncing, handlers: h }: Props) {
  const { t } = useTranslation();
  const [panelWidth, setPanelWidth] = useState(420);
  // Keep each pane's panels mounted once opened, so their state (directory, transfers)
  // survives switching panels and panes.
  const [mounted, setMounted] = useState<Record<number, SidePanel[]>>({});
  const pageRef = useRef<HTMLDivElement>(null);
  const focused = focusedPane(tab);
  const panelHere = tab.sidePanel !== null && focused.protocol === "ssh";
  if (panelHere && !mounted[focused.key]?.includes(tab.sidePanel!)) {
    setMounted({ ...mounted, [focused.key]: [...(mounted[focused.key] ?? []), tab.sidePanel!] });
  }

  const startResize = (e: ReactMouseEvent) => {
    const rect = pageRef.current!.getBoundingClientRect();
    dragHorizontally(e, (x) => setPanelWidth(Math.max(MIN_PANEL, Math.min(rect.right - x, rect.width - MIN_TERMINAL))));
  };

  const panels = (pane: Pane) => {
    const shown = (panel: SidePanel) => pane.key === focused.key && tab.sidePanel === panel;
    const connected = pane.status === "connected" && pane.sessionId != null;
    const list = mounted[pane.key] ?? [];
    const profile = h.profileOf(pane);
    return (
      <Fragment key={pane.key}>
        {list.includes("files") && (
          <div className="side-panel-page" style={{ display: shown("files") ? undefined : "none" }}>
            <SftpPanel sessionId={pane.sessionId} connected={connected} active={active && shown("files")} />
          </div>
        )}
        {list.includes("forwards") && (
          <div className="side-panel-page" style={{ display: shown("forwards") ? undefined : "none" }}>
            <ForwardsPanel
              sessionId={pane.sessionId}
              connected={connected}
              profile={profile}
              quick={pane.target.kind === "quick"}
              states={pane.forwards}
              onProfileChanged={h.onProfileChanged}
            />
          </div>
        )}
      </Fragment>
    );
  };

  const anyMounted = tab.panes.some((pane) => (mounted[pane.key] ?? []).length > 0);

  return (
    <div className={`tab-page${active ? " active" : ""}`} ref={pageRef}>
      <div className="pane-area">
        {tab.panes.map((pane) => {
          const profile = h.profileOf(pane);
          return (
            <div
              key={pane.key}
              className={`pane${pane.key === focused.key ? " focused" : ""}${syncing === pane.key ? " syncing" : ""}`}
              onMouseDownCapture={() => h.onFocus(pane.key)}
              onFocus={() => h.onFocus(pane.key)}
            >
              <TerminalView
                target={pane.target}
                shareFrom={pane.shareFrom}
                reconnectKey={pane.reconnectKey}
                active={active && pane.key === focused.key}
                autoReconnect={profile?.autoReconnect ?? true}
                onStatus={(status) => h.onStatus(pane.key, status)}
                onExited={(status) => h.onExited(pane.key, status)}
                onSession={(id) => h.onSession(pane.key, id)}
                onForward={(ruleId, state) => h.onForward(pane.key, ruleId, state)}
                onTitle={(title) => h.onTitle(pane.key, title)}
                onInput={(data) => h.onInput(pane.key, data)}
                menuItems={h.menuItems(pane)}
                logOpen={h.logOpen(pane)}
                onLog={(path) => h.onLog(pane.key, path)}
                appearance={profile?.appearance}
                loginCommands={profile?.loginCommands ?? []}
              />
            </div>
          );
        })}
      </div>
      {(anyMounted || tab.sidePanel) && (
        <div className="side-panel" style={{ width: panelWidth, display: tab.sidePanel ? undefined : "none" }}>
          <div className="splitter" onMouseDown={startResize} />
          {tab.panes.map(panels)}
          {tab.sidePanel && !panelHere && <div className="side-panel-empty">{t("tabs.panelUnavailable")}</div>}
        </div>
      )}
    </div>
  );
}
