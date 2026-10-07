import { useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

import type { ForwardState, Profile, SessionId } from "../lib/api";
import { ForwardsPanel } from "./ForwardsPanel";
import { SftpPanel } from "./SftpPanel";
import type { SidePanel, Tab } from "./TabBar";
import { TerminalView, type SessionStatus } from "./TerminalView";

interface Props {
  tab: Tab;
  /** Undefined if the profile was deleted while the tab is open. */
  profile: Profile | undefined;
  active: boolean;
  onStatus(status: SessionStatus): void;
  onForward(ruleId: string, state: ForwardState): void;
  onProfileChanged(profile: Profile): void;
}

const MIN_PANEL = 280;
const MIN_TERMINAL = 240;

/** One tab's content: the terminal plus a side panel (files or port forwards) on the same connection. */
export function SessionPane({ tab, profile, active, onStatus, onForward, onProfileChanged }: Props) {
  const [sessionId, setSessionId] = useState<SessionId | null>(null);
  const [panelWidth, setPanelWidth] = useState(420);
  // Keep panels mounted once opened so their state (directory, transfers) survives switching.
  const [mounted, setMounted] = useState<SidePanel[]>([]);
  const paneRef = useRef<HTMLDivElement>(null);
  if (tab.sidePanel && !mounted.includes(tab.sidePanel)) setMounted([...mounted, tab.sidePanel]);
  const connected = tab.status === "connected" && sessionId != null;

  const startResize = (e: ReactMouseEvent) => {
    e.preventDefault();
    const rect = paneRef.current!.getBoundingClientRect();
    const onMove = (ev: MouseEvent) => {
      const width = rect.right - ev.clientX;
      setPanelWidth(Math.max(MIN_PANEL, Math.min(width, rect.width - MIN_TERMINAL)));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.classList.remove("resizing");
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    document.body.classList.add("resizing");
  };

  const page = (panel: SidePanel) => ({ display: tab.sidePanel === panel ? undefined : "none" });

  return (
    <div className={`session-pane${active ? " active" : ""}`} ref={paneRef}>
      <TerminalView
        profileId={tab.profileId}
        active={active}
        onStatus={onStatus}
        onSession={setSessionId}
        onForward={onForward}
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
