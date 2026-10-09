import type { LogOpen, Profile, SessionId, ZmodemPhase } from "./api";
import { PaneSession, type SessionConfig, type TerminalPort } from "./paneSession";
import { findPane, targetProtocol, type Pane } from "./panes";
import { activeTabOf, type TabStore } from "./tabs";

/** What a pane's terminal hears of its session, besides what goes into the tab store. */
export interface PaneView {
  port: TerminalPort;
  zmodem(phase: ZmodemPhase): void;
  /** The backend session changed: a new one, or none (null). */
  session(id: SessionId | null): void;
}

export interface SessionRegistry {
  /** Creates the session of a pane whose terminal is shown; call `connect` on it to start. */
  attach(key: number, view: PaneView): PaneSession;
  /** The terminal is gone: its session closes. */
  detach(key: number, session: PaneSession): void;
  get(key: number): PaneSession | undefined;
  /** Closes the pane's session and connects again. */
  reconnect(key: number): void;
}

interface Options {
  /** The saved sessions, for a pane's settings at each connection. */
  profiles(): Profile[];
  t: SessionConfig["t"];
  /** Tells screen readers (see `announce`). */
  announce(message: string): void;
}

/** How a pane's next session starts logging (see `Pane.logPath`). */
const logOpen = (pane: Pane): LogOpen =>
  pane.logPath ? { mode: "append", path: pane.logPath } : pane.logStopped ? { mode: "off" } : { mode: "auto" };

/**
 * The sessions of the panes, by pane key. Each reports its status, backend session,
 * forwarding and log into the tab store; a local shell that exits cleanly (`exit`, Ctrl+D)
 * closes its pane, as in Terminal.app.
 */
export function createSessionRegistry(store: TabStore, { profiles, t, announce }: Options): SessionRegistry {
  const sessions = new Map<number, PaneSession>();

  return {
    attach(key, view) {
      const found = findPane(store.get().tabs, key);
      if (!found) throw new Error(`no pane ${key}`);
      // The last known state, for callbacks that come after the pane has closed.
      let last = found.pane;
      const pane = () => (last = findPane(store.get().tabs, key)?.pane ?? last);
      const profile = () => {
        const { target } = pane();
        return target.kind === "profile" ? profiles().find((p) => p.id === target.profileId) : undefined;
      };
      const update = (patch: Partial<Pane>) => store.dispatch({ type: "updatePane", key, patch });
      // A remote pane out of sight connecting or disconnecting; the one with the keyboard
      // says so in its terminal.
      const announceStatus = (status: "connected" | "closed") => {
        const current = pane();
        if (current.target.kind === "local" || current.status === status) return;
        if (activeTabOf(store.get())?.focused === key) return;
        announce(t(status === "connected" ? "announce.connected" : "announce.disconnected", { name: current.title }));
      };

      const session = new PaneSession(
        view.port,
        {
          target: () => pane().target,
          logOpen: () => logOpen(pane()),
          loginCommands: () => profile()?.loginCommands ?? [],
          autoReconnect: () => profile()?.autoReconnect ?? true,
          t,
        },
        {
          // Forward events can precede "connected" (auto-start runs right after
          // authentication), so states are only reset when a connection attempt starts or ends.
          status: (status) => {
            if (status === "connecting") {
              store.dispatch({ type: "connecting", key, protocol: targetProtocol(pane().target, profiles()) });
            } else {
              announceStatus(status);
              update(status === "connected" ? { status } : { status, forwards: {} });
            }
          },
          session: (id) => {
            update({ sessionId: id });
            view.session(id);
          },
          forward: (ruleId, state) =>
            store.dispatch({ type: "updatePane", key, patch: (p) => ({ forwards: { ...p.forwards, [ruleId]: state } }) }),
          zmodem: view.zmodem,
          log: (path) => update({ logPath: path }),
          exited: (status) => {
            if (pane().target.kind === "local" && status === 0) store.dispatch({ type: "close", keys: [key] });
          },
        },
        found.pane.shareFrom,
      );
      sessions.set(key, session);
      return session;
    },
    detach(key, session) {
      session.dispose();
      if (sessions.get(key) === session) sessions.delete(key);
    },
    get: (key) => sessions.get(key),
    reconnect: (key) => sessions.get(key)?.reconnect(),
  };
}
