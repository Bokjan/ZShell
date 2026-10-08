import { useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

import type { ForwardState, LogOpen, Profile, SessionId } from "../lib/api";
import type { MenuItem } from "./ContextMenu";
import { dragHorizontally } from "../lib/drag";
import { ForwardsPanel } from "./ForwardsPanel";
import { SftpPanel } from "./SftpPanel";
import type { SidePanel, Tab } from "./TabBar";
import { TerminalView, type SessionStatus } from "./TerminalView";

interface Props {
  tab: Tab;
  /** Undefined for local terminals, and if the profile was deleted while the tab is open. */
  profile: Profile | undefined;
  active: boolean;
  onStatus(status: SessionStatus): void;
  onExited(status: number | null): void;
  onSession(id: SessionId | null): void;
  onForward(ruleId: string, state: ForwardState): void;
  onTitle(title: string): void;
  /** What the user typed in the terminal (see `TerminalView`). */
  onInput(data: string): void;
  /** Input typed here is sent to other tabs too (the compose bar's sync); shown on the pane. */
  syncing: boolean;
  /** Added to the end of the terminal's context menu (quick commands). */
  menuItems: MenuItem[];
  logOpen: LogOpen;
  onLog(path: string | null): void;
  onProfileChanged(profile: Profile): void;
}

const MIN_PANEL = 280;
const MIN_TERMINAL = 240;

/**
 * One tab's content: the terminal plus, for SSH tabs, a side panel (files or port forwards)
 * on the same connection.
 */
export function SessionPane({
  tab,
  profile,
  active,
  onStatus,
  onExited,
  onSession,
  onForward,
  onTitle,
  onInput,
  syncing,
  menuItems,
  logOpen,
  onLog,
  onProfileChanged,
}: Props) {
  const { sessionId } = tab;
  const [panelWidth, setPanelWidth] = useState(420);
  // Keep panels mounted once opened so their state (directory, transfers) survives switching.
  const [mounted, setMounted] = useState<SidePanel[]>([]);
  const paneRef = useRef<HTMLDivElement>(null);
  if (tab.sidePanel && !mounted.includes(tab.sidePanel)) setMounted([...mounted, tab.sidePanel]);
  const connected = tab.status === "connected" && sessionId != null;

  const startResize = (e: ReactMouseEvent) => {
    const rect = paneRef.current!.getBoundingClientRect();
    dragHorizontally(e, (x) => setPanelWidth(Math.max(MIN_PANEL, Math.min(rect.right - x, rect.width - MIN_TERMINAL))));
  };

  const page = (panel: SidePanel) => ({ display: tab.sidePanel === panel ? undefined : "none" });

  return (
    <div className={`session-pane${active ? " active" : ""}${syncing ? " syncing" : ""}`} ref={paneRef}>
      <TerminalView
        target={tab.target}
        shareFrom={tab.shareFrom}
        reconnectKey={tab.reconnectKey}
        active={active}
        autoReconnect={profile?.autoReconnect ?? true}
        onStatus={onStatus}
        onExited={onExited}
        onSession={onSession}
        onForward={onForward}
        onTitle={onTitle}
        onInput={onInput}
        menuItems={menuItems}
        logOpen={logOpen}
        onLog={onLog}
        appearance={profile?.appearance}
        loginCommands={profile?.loginCommands ?? []}
      />
      {mounted.length > 0 && (
        <div className="side-panel" style={{ width: panelWidth, display: tab.sidePanel ? undefined : "none" }}>
          <div className="splitter" onMouseDown={startResize} />
          {mounted.includes("files") && (
            <div className="side-panel-page" style={page("files")}>
              <SftpPanel sessionId={sessionId} connected={connected} active={active && tab.sidePanel === "files"} />
            </div>
          )}
          {mounted.includes("forwards") && (
            <div className="side-panel-page" style={page("forwards")}>
              <ForwardsPanel
                sessionId={sessionId}
                connected={connected}
                profile={profile}
                quick={tab.target.kind === "quick"}
                states={tab.forwards}
                onProfileChanged={onProfileChanged}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
