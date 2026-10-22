import { playBell, type LogOpen, type Profile, type SessionId, type ZmodemPhase } from "../ipc/api";
import type { TerminalSettings } from "../ipc/bindings";
import { windows } from "../ipc/windows";
import { isAppLocked } from "./appLock";
import { PaneSession, type SessionConfig, type TerminalPort } from "./paneSession";
import { closesOnExit, findPane, targetProfile, targetProtocol, targetTermType, type Arrival, type Pane, type Snapshot } from "./panes";
import { activeTabOf, type TabsState, type TabStore } from "./tabs";

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
  /** Pastes into the terminal, for another pane syncing its input with this one. */
  paste: PasteTarget;
}

/** A pane that text can be pasted into, as its terminal's mode requires. */
export interface PasteTarget {
  /** Whether the program in it takes bracketed pastes (inserting lines without running them). */
  bracketed(): boolean;
  paste(text: string): void;
}

/**
 * What a pane's session asks of those who own the tabs and the window, which they hear
 * through `SessionRegistry.on`: the registry is made first, and they are made with it.
 */
export interface RegistryEvents {
  /** The pane's session moved to another window, which has its screen now. */
  movedAway(key: number): void;
  /** The session of pane `key`, which came `from` another window, couldn't move here. */
  stayed(key: number, from: Arrival["from"]): void;
  /** Close pane `key` (or its tab, if it is the only pane) as the user would, asking if needed. */
  close(key: number): void;
}

export interface SessionRegistry {
  /** Creates the session of a pane whose terminal is shown; call `connect` on it to start. */
  attach(key: number, view: PaneView): PaneSession;
  /** The terminal is gone: its session closes. */
  detach(key: number, session: PaneSession): void;
  get(key: number): PaneSession | undefined;
  /** The pane's terminal, while it is shown. */
  screen(key: number): TerminalScreen | undefined;
  /**
   * Calls `listener` when a pane's session and terminal come or go (what `get` and `screen`
   * return), for `useSyncExternalStore`; returns what stops it.
   */
  subscribe(listener: () => void): () => void;
  /** Closes the pane's session and connects again. */
  reconnect(key: number): void;
  /** Gives the pane's terminal the keyboard focus. */
  focus(key: number): void;
  /** Whether text is selected in the pane's terminal. */
  hasSelection(key: number): boolean;
  /** Pasting into the pane's terminal, while it is shown. */
  pasteTarget(key: number): PasteTarget | undefined;
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
  /** Calls `handler` on `event` (see `RegistryEvents`); returns what stops it. */
  on<E extends keyof RegistryEvents>(event: E, handler: RegistryEvents[E]): () => void;
}

interface Options {
  /** The saved sessions, for a pane's settings at each connection. */
  profiles(): Profile[];
  t: SessionConfig["t"];
  /** Tells screen readers (see `announce`). */
  announce(message: string): void;
  /** Whether tabs in the background are marked when they have new output (a setting). */
  markOutput(): boolean;
  /** What a terminal's bell does (settings). */
  bell(): Pick<TerminalSettings, "bellAlert" | "bellSound" | "bellFlash">;
  /** Whether programs can notify the user (a setting). */
  notifications(): boolean;
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
  { profiles, t, announce, markOutput, bell, notifications }: Options,
): SessionRegistry {
  const sessions = new Map<number, PaneSession>();
  const views = new Map<number, PaneView>();
  const zmodemPhases = new Map<number, ZmodemPhase>();
  const listeners = new Set<() => void>();
  const changed = () => {
    for (const listener of listeners) listener();
  };
  const handlers: { [E in keyof RegistryEvents]: Set<RegistryEvents[E]> } = { movedAway: new Set(), stayed: new Set(), close: new Set() };
  const emit = <E extends keyof RegistryEvents>(event: E, ...args: Parameters<RegistryEvents[E]>) => {
    for (const handler of handlers[event]) (handler as (...args: Parameters<RegistryEvents[E]>) => void)(...args);
  };

  return {
    attach(key, view) {
      const found = findPane(store.get().tabs, key);
      if (!found) throw new Error(`no pane ${key}`);
      // The last known state, for callbacks that come after the pane has closed.
      let last = found.pane;
      // The tab state `output` last looked at.
      let outputChecked: TabsState | undefined;
      const pane = () => (last = findPane(store.get().tabs, key)?.pane ?? last);
      const profile = () => targetProfile(pane().target, profiles());
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
          close: () => emit("close", key),
          retry: (at) => update({ retryAt: at }),
          // Often: only a tab in the background that isn't marked yet changes the store. The
          // same state as last time gives the same answer.
          output: () => {
            if (!markOutput()) return;
            const state = store.get();
            if (state === outputChecked) return;
            outputChecked = state;
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
          // A tab in the background whose program fails tells screen readers; the tab shows it.
          progress: (progress) => {
            const failed = progress?.state === "error" && pane().progress?.state !== "error";
            update({ progress });
            if (failed && activeTabOf(store.get())?.panes.every((p) => p.key !== key))
              announce(t("announce.progressFailed", { name: pane().title }));
          },
          // Not while the user sees the pane's tab. A tab in the background is marked, as for
          // a bell; out of the window, the system shows the notification. While the app is
          // locked, without its text or the pane's title, which the lock screen hides.
          notify: ({ title, body }) => {
            if (!notifications()) return;
            const state = store.get();
            const tab = state.tabs.find((tab) => tab.panes.some((p) => p.key === key));
            const locked = isAppLocked();
            const seen = document.hasFocus() && !locked;
            if (!tab || (seen && tab.key === state.activeKey)) return;
            const name = pane().title;
            if (tab.key !== state.activeKey) store.dispatch({ type: "attention", key, kind: "notice" });
            if (seen) announce(t("announce.notice", { name, text: [title, body].filter(Boolean).join(": ") }));
            else if (locked) windows.notify(t("appLock.notice"), "");
            else windows.notify(title || name, body);
          },
          arrived: () => update({ arrival: undefined }),
          movedAway: () => emit("movedAway", key),
          stayed: () => {
            if (found.pane.arrival) emit("stayed", key, found.pane.arrival.from);
          },
        },
        found.pane.shareFrom,
        found.pane.arrival,
      );
      sessions.set(key, session);
      views.set(key, view);
      changed();
      return session;
    },
    detach(key, session) {
      session.dispose();
      if (sessions.get(key) === session) {
        sessions.delete(key);
        views.delete(key);
        zmodemPhases.delete(key);
        changed();
      }
    },
    get: (key) => sessions.get(key),
    screen: (key) => views.get(key)?.screen,
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    reconnect: (key) => sessions.get(key)?.reconnect(),
    focus: (key) => views.get(key)?.focus(),
    hasSelection: (key) => views.get(key)?.hasSelection() ?? false,
    pasteTarget: (key) => views.get(key)?.paste,
    snapshot: (key) => views.get(key)?.port.snapshot(),
    inZmodem: (key) => (zmodemPhases.get(key) ?? "idle") !== "idle",
    arriving: (key) => sessions.get(key)?.isArriving ?? false,
    suspend: (key) => sessions.get(key)?.suspend(),
    resume: (key) => sessions.get(key)?.resume(),
    on(event, handler) {
      handlers[event].add(handler);
      return () => void handlers[event].delete(handler);
    },
  };
}
