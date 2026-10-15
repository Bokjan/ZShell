import { playBell, type LogOpen, type Profile, type SessionId, type ZmodemPhase } from "../ipc/api";
import type { TerminalSettings } from "../ipc/bindings";
import { windows } from "../ipc/windows";
import { PaneSession, type SessionConfig, type TerminalPort } from "./paneSession";
import { closesOnExit, findPane, targetProtocol, targetTermType, type Arrival, type Pane, type Snapshot } from "./panes";
import { activeTabOf, type TabStore } from "./tabs";

/** A pane's terminal as the status bar shows it. */
export interface TerminalScreen {
  size(): { cols: number; rows: number };
  /** Where the cursor is on the screen, counted from 1. */
  cursor(): { row: number; column: number };
  /** Calls `listener` when the size or the cursor changes, which can be often. */
  subscribe(listener: () => void): () => void;
}

/** What a pane's terminal hears of its session, besides what goes into the tab store. */
export interface PaneView {
  port: TerminalPort;
  screen: TerminalScreen;
  zmodem(phase: ZmodemPhase): void;
  /** The backend session changed: a new one, or none (null). */
  session(id: SessionId | null): void;
  /** Gives the terminal the keyboard focus. */
  focus(): void;
  /** Whether text is selected in the terminal. */
  hasSelection(): boolean;
  /** Flashes the terminal: a visual bell. */
  flash(): void;
}

export interface SessionRegistry {
  /** Creates the session of a pane whose terminal is shown; call `connect` on it to start. */
  attach(key: number, view: PaneView): PaneSession;
  /** The terminal is gone: its session closes. */
  detach(key: number, session: PaneSession): void;
  get(key: number): PaneSession | undefined;
  /** The pane's terminal, while it is shown. */
  screen(key: number): TerminalScreen | undefined;
  /** Closes the pane's session and connects again. */
  reconnect(key: number): void;
  /** Gives the pane's terminal the keyboard focus. */
  focus(key: number): void;
  /** Whether text is selected in the pane's terminal. */
  hasSelection(key: number): boolean;
  /** What the pane's terminal shows, while it is shown. */
  snapshot(key: number): Snapshot | undefined;
  /** Whether a ZMODEM transfer runs in the pane, or waits for an answer. */
  inZmodem(key: number): boolean;
  /** Whether the pane's session is still moving here from another window. */
  arriving(key: number): boolean;
  /** The pane (without a session) moves to another window: nothing connects here meanwhile. */
  suspend(key: number): void;
  /** The pane couldn't move: it may connect again. */
  resume(key: number): void;
}

interface Options {
  /** The saved sessions, for a pane's settings at each connection. */
  profiles(): Profile[];
  t: SessionConfig["t"];
  /** Tells screen readers (see `announce`). */
  announce(message: string): void;
  /** The pane's session moved to another window, which has its screen now. */
  movedAway(key: number): void;
  /** The session of pane `key`, which came `from` another window, couldn't move here. */
  stayed(key: number, from: Arrival["from"]): void;
  /** Closes pane `key` (or its tab, if it is the only pane) as the user would, asking if needed. */
  close(key: number): void;
  /** Whether tabs in the background are marked when they have new output (a setting). */
  markOutput(): boolean;
  /** What a terminal's bell does (settings). */
  bell(): Pick<TerminalSettings, "bellAlert" | "bellSound" | "bellFlash">;
}

/** How a pane's next session starts logging (see `Pane.logPath`). */
const logOpen = (pane: Pane): LogOpen =>
  pane.logPath ? { mode: "append", path: pane.logPath } : pane.logStopped ? { mode: "off" } : { mode: "auto" };

/**
 * The sessions of the panes, by pane key. Each reports its status, backend session,
 * forwarding and log into the tab store; a shell that the user exits closes its pane (see
 * `closesOnExit`).
 */
export function createSessionRegistry(
  store: TabStore,
  { profiles, t, announce, movedAway, stayed, close, markOutput, bell }: Options,
): SessionRegistry {
  const sessions = new Map<number, PaneSession>();
  const views = new Map<number, PaneView>();
  const zmodemPhases = new Map<number, ZmodemPhase>();

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
              const { target } = pane();
              store.dispatch({
                type: "connecting",
                key,
                protocol: targetProtocol(target, profiles()),
                termType: targetTermType(target, profiles()),
              });
            } else {
              announceStatus(status);
              update(status === "connected" ? { status } : { status, forwards: {} });
            }
          },
          session: (id) => {
            zmodemPhases.delete(key);
            update({ sessionId: id });
            view.session(id);
          },
          forward: (ruleId, state) =>
            store.dispatch({ type: "updatePane", key, patch: (p) => ({ forwards: { ...p.forwards, [ruleId]: state } }) }),
          zmodem: (phase) => {
            zmodemPhases.set(key, phase);
            view.zmodem(phase);
          },
          log: (path) => update({ logPath: path }),
          exited: (status, signal, typed) => {
            const current = pane();
            if (!closesOnExit(current, status, signal, typed)) return false;
            if (activeTabOf(store.get())?.focused !== key) announce(t("announce.closed", { name: current.title }));
            store.dispatch({ type: "close", keys: [key] });
            return true;
          },
          close: () => close(key),
          retry: (at) => update({ retryAt: at }),
          // Often: only a tab in the background that isn't marked yet changes the store.
          output: () => {
            if (!markOutput()) return;
            const state = store.get();
            const tab = state.tabs.find((tab) => tab.panes.some((p) => p.key === key));
            if (tab && tab.key !== state.activeKey && !tab.attention) store.dispatch({ type: "attention", key, kind: "output" });
          },
          // One that answers typing (a failed completion) only sounds and flashes: the user is there.
          bell: (echo) => {
            const { bellAlert, bellSound, bellFlash } = bell();
            if (bellSound) void playBell().catch(console.error);
            if (bellFlash) view.flash();
            if (!bellAlert || echo) return;
            const state = store.get();
            const tab = state.tabs.find((tab) => tab.panes.some((p) => p.key === key));
            if (tab && tab.key !== state.activeKey && tab.attention !== "bell") {
              store.dispatch({ type: "attention", key, kind: "bell" });
              announce(t("announce.bell", { name: pane().title }));
            }
            if (!document.hasFocus()) windows.requestAttention();
          },
          arrived: () => update({ arrival: undefined }),
          movedAway: () => movedAway(key),
          stayed: () => {
            if (found.pane.arrival) stayed(key, found.pane.arrival.from);
          },
        },
        found.pane.shareFrom,
        found.pane.arrival,
      );
      sessions.set(key, session);
      views.set(key, view);
      return session;
    },
    detach(key, session) {
      session.dispose();
      if (sessions.get(key) === session) {
        sessions.delete(key);
        views.delete(key);
        zmodemPhases.delete(key);
      }
    },
    get: (key) => sessions.get(key),
    screen: (key) => views.get(key)?.screen,
    reconnect: (key) => sessions.get(key)?.reconnect(),
    focus: (key) => views.get(key)?.focus(),
    hasSelection: (key) => views.get(key)?.hasSelection() ?? false,
    snapshot: (key) => views.get(key)?.port.snapshot(),
    inZmodem: (key) => (zmodemPhases.get(key) ?? "idle") !== "idle",
    arriving: (key) => sessions.get(key)?.isArriving ?? false,
    suspend: (key) => sessions.get(key)?.suspend(),
    resume: (key) => sessions.get(key)?.resume(),
  };
}
