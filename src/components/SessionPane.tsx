import { useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

import type { SessionId } from "../lib/api";
import { SftpPanel } from "./SftpPanel";
import type { Tab } from "./TabBar";
import { TerminalView, type SessionStatus } from "./TerminalView";

interface Props {
  tab: Tab;
  active: boolean;
  onStatus(status: SessionStatus): void;
}

const MIN_PANEL = 280;
const MIN_TERMINAL = 240;

/** One tab's content: the terminal plus its (optional) SFTP panel on the same connection. */
export function SessionPane({ tab, active, onStatus }: Props) {
  const [sessionId, setSessionId] = useState<SessionId | null>(null);
  const [panelWidth, setPanelWidth] = useState(420);
  // Keep the panel mounted once opened so its directory and transfers survive toggling.
  const [panelMounted, setPanelMounted] = useState(false);
  const paneRef = useRef<HTMLDivElement>(null);
  if (tab.filesOpen && !panelMounted) setPanelMounted(true);

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

  return (
    <div className={`session-pane${active ? " active" : ""}`} ref={paneRef}>
      <TerminalView profileId={tab.profileId} active={active} onStatus={onStatus} onSession={setSessionId} />
      {panelMounted && (
        <div className="side-panel" style={{ width: panelWidth, display: tab.filesOpen ? undefined : "none" }}>
          <div className="splitter" onMouseDown={startResize} />
          <SftpPanel
            sessionId={sessionId}
            connected={tab.status === "connected" && sessionId != null}
            active={active && tab.filesOpen}
          />
        </div>
      )}
    </div>
  );
}
