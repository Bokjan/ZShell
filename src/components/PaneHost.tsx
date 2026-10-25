import { memo, useEffect, useState } from "react";
import { createPortal } from "react-dom";

import { DialogsHidden } from "../hooks/dialogs";
import { useStableProps } from "../hooks/stableProps";
import { useRegistered } from "../hooks/stores";
import type { Profile } from "../ipc/api";
import { focusedPane, targetProfile, type Pane, type SidePanel, type Tab } from "../lib/panes";
import { ForwardsPanel } from "./ForwardsPanel";
import { SftpPanel } from "./SftpPanel";
import type { PaneHandlers } from "./TabPage";
import { TerminalView } from "./TerminalView";

interface Props {
  tabs: Tab[];
  activeKey: number | null;
  profiles: Profile[];
  /** Stable: see `PaneHandlers`. */
  handlers: PaneHandlers;
}

/**
 * Every pane's terminal and side panels, wherever the pane is: drawn into the pane's hosts,
 * which its tab's page puts in place (see `PaneHosts`). Kept here, in one list by pane key,
 * a pane's terminal stays mounted as the pane moves from tab to tab.
 */
export function PaneHostList({ tabs, activeKey, profiles, handlers }: Props) {
  const { hosts } = handlers;
  // Once the panes have rendered: StrictMode's second mount finds the same hosts.
  const open = new Set(tabs.flatMap((tab) => tab.panes.map((pane) => pane.key)));
  useEffect(() => hosts.retain(open));

  return tabs.flatMap((tab) => {
    const focused = focusedPane(tab).key;
    return tab.panes.map((pane) => (
      <PaneHost
        key={pane.key}
        pane={pane}
        profile={targetProfile(pane.target, profiles)}
        visible={tab.key === activeKey}
        focused={pane.key === focused}
        sidePanel={tab.sidePanel}
        handlers={handlers}
      />
    ));
  });
}

interface PaneHostProps {
  pane: Pane;
  /** The saved session the pane runs, which its appearance and forwarding rules come from. */
  profile: Profile | undefined;
  /** Its tab is shown. */
  visible: boolean;
  /** It has its tab's focus, and the tab's side panel shows its connection. */
  focused: boolean;
  /** The side panel its tab shows, if any. */
  sidePanel: SidePanel | null;
  handlers: PaneHandlers;
}

/**
 * One pane's terminal and side panels. Memoized: it renders again only when the pane or its
 * place changes, not each time some tab does.
 */
const PaneHost = memo(function PaneHost(props: PaneHostProps) {
  useStableProps("PaneHost", props, ["handlers"]);
  const { pane, profile, visible, focused, sidePanel, handlers: h } = props;
  const { key } = pane;
  const [terminalHost] = useState(() => h.hosts.terminal(key));
  const [panelHost] = useState(() => h.hosts.panels(key));
  // Each panel stays mounted once opened, so that its state (folder, transfers) survives
  // switching panels and panes.
  const [mounted, setMounted] = useState<SidePanel[]>([]);
  if (focused && sidePanel !== null && pane.protocol === "ssh" && !mounted.includes(sidePanel)) {
    setMounted([...mounted, sidePanel]);
  }
  const shown = (panel: SidePanel) => focused && sidePanel === panel;
  const connected = pane.status === "connected" && pane.sessionId != null;
  const session = useRegistered(h.sessions, () => h.sessions.get(key));

  return (
    <>
      {createPortal(
        // Dialogs of a pane in the background (a paste to confirm) wait for its tab to be shown.
        <DialogsHidden.Provider value={!visible}>
          <TerminalView
            paneKey={key}
            sessions={h.sessions}
            visible={visible}
            active={visible && focused}
            onTitle={(title) => h.onTitle(key, title)}
            onInput={(data) => h.onInput(key, data)}
            pasteTargets={() => h.pasteTargets(key)}
            menuItems={() => h.menuItems(key)}
            local={pane.protocol === "local"}
            appearance={profile?.appearance}
          />
        </DialogsHidden.Provider>,
        terminalHost,
      )}
      {mounted.length > 0 &&
        createPortal(
          <>
            {mounted.includes("files") && (
              <div className="side-panel-page" style={{ display: shown("files") ? undefined : "none" }}>
                <DialogsHidden.Provider value={!visible || !shown("files")}>
                  <SftpPanel
                    paneKey={key}
                    sessionId={pane.sessionId}
                    connected={connected}
                    active={visible && shown("files")}
                    traffic={session?.sftpTraffic}
                    onTransfers={(count) => h.onTransfers(key, count)}
                    onEdits={(count) => h.onEdits(key, count)}
                    onQuestions={(count) => h.onQuestions(key, count)}
                  />
                </DialogsHidden.Provider>
              </div>
            )}
            {mounted.includes("forwards") && (
              <div className="side-panel-page" style={{ display: shown("forwards") ? undefined : "none" }}>
                <DialogsHidden.Provider value={!visible || !shown("forwards")}>
                  <ForwardsPanel
                    sessionId={pane.sessionId}
                    connected={connected}
                    profile={profile}
                    quick={pane.target.kind === "quick"}
                    states={pane.forwards}
                    traffic={session?.forwardTraffic}
                  />
                </DialogsHidden.Provider>
              </div>
            )}
          </>,
          panelHost,
        )}
    </>
  );
});
