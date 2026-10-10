import type { TFunction } from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CommandError, Session, SessionEvent, SessionTarget, openSession } from "./api";
import { LOGIN_COMMAND_IDLE_MS, PaneSession, type SessionConfig, type SessionEvents, type TerminalPort } from "./paneSession";

/** One call of the fake `openSession`. */
interface Opened {
  target: SessionTarget;
  shareFrom: number | undefined;
  carry: string[];
  output(text: string): void;
  event(event: SessionEvent): void;
  /** Settles the call: a session, or an error. */
  resolve(): Session & { writes: string[]; closed: boolean };
  reject(error: unknown): void;
}

const lost = (error: CommandError | null = null): SessionEvent => ({ type: "closed", reason: "lost", error, status: null });
const failed = (error: CommandError | null = null): SessionEvent => ({ type: "closed", reason: "failed", error, status: null });

let nextId = 1;

function setup(config: Partial<SessionConfig> = {}, shareFrom?: number) {
  const opened: Opened[] = [];
  const open = ((target, _size, onOutput, onEvent, share, _log, carry = []) =>
    new Promise<Session>((resolve, reject) => {
      opened.push({
        target,
        shareFrom: share,
        carry,
        output: (text) => onOutput(new TextEncoder().encode(text)),
        event: onEvent,
        resolve: () => {
          const session = {
            id: nextId++,
            writes: [] as string[],
            closed: false,
            write: async (data: string) => void session.writes.push(data),
            resize: async () => {},
            ack: async () => {},
            close: async () => void (session.closed = true),
          };
          resolve(session);
          return session;
        },
        reject,
      });
    })) as typeof openSession;
  let line = "";
  const written: string[] = [];
  const port: TerminalPort = {
    write: (data, done) => {
      written.push(typeof data === "string" ? data : new TextDecoder().decode(data));
      done?.();
    },
    size: () => ({ cols: 80, rows: 24 }),
    lineBeforeCursor: () => line,
    alternateScreen: () => false,
  };
  const events = {
    status: vi.fn(),
    session: vi.fn(),
    forward: vi.fn(),
    zmodem: vi.fn(),
    log: vi.fn(),
    exited: vi.fn(),
  } satisfies SessionEvents;
  const pane = new PaneSession(
    port,
    {
      target: () => ({ kind: "quick", protocol: "ssh", username: "", host: "example", port: 22 }),
      logOpen: () => ({ mode: "auto" }),
      loginCommands: () => [],
      autoReconnect: () => true,
      t: ((key: string) => key) as unknown as TFunction,
      ...config,
    },
    events,
    shareFrom,
    open,
  );
  return {
    pane,
    opened,
    events,
    written,
    setLine: (text: string) => (line = text),
    /** What was written to the terminal, as one string. */
    screen: () => written.join(""),
  };
}

/** Lets settled promises run their callbacks. */
const settle = () => vi.advanceTimersByTimeAsync(0);

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("window", new EventTarget());
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("connecting", () => {
  it("reports the session and its status", async () => {
    const { pane, opened, events } = setup();
    pane.connect();
    expect(events.status).toHaveBeenLastCalledWith("connecting");
    const session = opened[0].resolve();
    await settle();
    expect(events.session).toHaveBeenLastCalledWith(session.id);
    expect(pane.id).toBe(session.id);
    opened[0].event({ type: "connected" });
    expect(events.status).toHaveBeenLastCalledWith("connected");
    expect(pane.input("ls\r")).toBe(true);
    await settle();
    expect(session.writes).toEqual(["ls\r"]);
  });

  it("shows why opening failed, and connects again on Enter", async () => {
    const { pane, opened, events, screen } = setup();
    pane.connect();
    opened[0].reject({ code: "net.refused", message: "Connection refused" });
    await settle();
    expect(events.status).toHaveBeenLastCalledWith("closed");
    expect(screen()).toContain("terminal.retryHint");
    expect(pane.isClosed).toBe(true);
    expect(pane.input("x")).toBe(false);
    expect(opened).toHaveLength(1);
    pane.input("\r");
    expect(opened).toHaveLength(2);
  });

  it("closes a session that opens after the pane was disposed", async () => {
    const { pane, opened } = setup();
    pane.connect();
    pane.dispose();
    const session = opened[0].resolve();
    await settle();
    expect(session.closed).toBe(true);
  });
});

describe("automatic reconnection", () => {
  it("retries a lost connection with growing delays, and stops once connected", async () => {
    const { pane, opened, screen } = setup();
    pane.connect();
    opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    opened[0].event(lost());
    expect(screen()).toContain("terminal.reconnectIn");
    await vi.advanceTimersByTimeAsync(1999);
    expect(opened).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(opened).toHaveLength(2);
    // Still down: the next attempt waits longer.
    opened[1].event(failed());
    await vi.advanceTimersByTimeAsync(3999);
    expect(opened).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(opened).toHaveLength(3);
    opened[2].resolve();
    await settle();
    opened[2].event({ type: "connected" });
    // Back to the shortest delay after a successful connection.
    opened[2].event(lost());
    await vi.advanceTimersByTimeAsync(2000);
    expect(opened).toHaveLength(4);
  });

  it("doesn't retry a first connection that fails, nor an authentication failure", async () => {
    const { pane, opened, screen } = setup();
    pane.connect();
    opened[0].event(failed());
    await vi.advanceTimersByTimeAsync(60_000);
    expect(opened).toHaveLength(1);
    expect(screen()).toContain("terminal.reconnectHint");
    pane.input("\r");
    opened[1].event({ type: "connected" });
    opened[1].event(lost({ code: "auth.failed", message: "", params: {} }));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(opened).toHaveLength(2);
  });

  it("doesn't retry when turned off for the session", async () => {
    const { pane, opened } = setup({ autoReconnect: () => false });
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event(lost());
    await vi.advanceTimersByTimeAsync(60_000);
    expect(opened).toHaveLength(1);
  });

  it("cancels the pending retry on Ctrl+C, and retries at once when the network is back", async () => {
    const { pane, opened, screen } = setup();
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event(lost());
    pane.input("\x03");
    expect(screen()).toContain("terminal.reconnectCancelled");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(opened).toHaveLength(1);

    pane.input("\r");
    opened[1].event({ type: "connected" });
    opened[1].event(lost());
    window.dispatchEvent(new Event("online"));
    expect(opened).toHaveLength(3);
  });

  it("never retries a local terminal", async () => {
    const { pane, opened, events, screen } = setup({ target: () => ({ kind: "local" }) });
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event({ type: "closed", reason: "exited", error: null, status: 0 });
    expect(events.exited).toHaveBeenCalledWith(0);
    expect(screen()).toContain("terminal.restartHint");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(opened).toHaveLength(1);
  });
});

describe("shared connections", () => {
  it("opens the first shell on the shared connection, and later ones anew", async () => {
    const { pane, opened } = setup({}, 7);
    pane.connect();
    expect(opened[0].shareFrom).toBe(7);
    opened[0].event({ type: "connected" });
    opened[0].event(lost());
    await vi.advanceTimersByTimeAsync(2000);
    expect(opened[1].shareFrom).toBeUndefined();
  });

  it("connects anew when the shared connection is gone", async () => {
    const { pane, opened } = setup({}, 7);
    pane.connect();
    opened[0].reject({ code: "session.notConnected", message: "" });
    await settle();
    expect(opened).toHaveLength(2);
    expect(opened[1].shareFrom).toBeUndefined();
  });

  it("connects anew when the shared connection dies before the shell is up", async () => {
    const { pane, opened } = setup({}, 7);
    pane.connect();
    opened[0].event(failed());
    expect(opened).toHaveLength(2);
    expect(opened[1].shareFrom).toBeUndefined();
  });
});

describe("reconnecting by hand", () => {
  it("closes the session and ignores what it still sends", async () => {
    const { pane, opened, events, screen } = setup();
    pane.connect();
    const first = opened[0].resolve();
    await settle();
    pane.reconnect();
    expect(events.session).toHaveBeenLastCalledWith(null);
    await settle();
    expect(first.closed).toBe(true);
    expect(opened).toHaveLength(2);
    opened[0].output("late output");
    opened[0].event(lost());
    expect(screen()).not.toContain("late output");
    expect(events.status).toHaveBeenLastCalledWith("connecting");
  });
});

describe("login commands", () => {
  it("types each command once the output pauses at a prompt", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["cd /srv", "ls"] });
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    setLine("Password:");
    opened[0].output("Password:");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual([]);
    setLine("$ ");
    opened[0].output("$ ");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["cd /srv\r"]);
    opened[0].output("$ ");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["cd /srv\r", "ls\r"]);
  });

  it("stops typing them on Ctrl+C", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["sleep 1"] });
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    setLine("$ ");
    pane.input("\x03");
    opened[0].output("$ ");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["\x03"]);
  });
});

describe("text from elsewhere", () => {
  it("goes only into a shell that is up", async () => {
    const { pane, opened } = setup();
    // Before the session exists, and while connecting (a passphrase prompt).
    expect(pane.send("ls\r")).toBe(false);
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    expect(pane.send("secret\r")).toBe(false);
    opened[0].event({ type: "connected" });
    expect(pane.send("ls\r")).toBe(true);
    await settle();
    expect(session.writes).toEqual(["ls\r"]);
    // Nor into a closed terminal, where Enter would connect again.
    opened[0].event(lost());
    expect(pane.send("\r")).toBe(false);
    expect(opened).toHaveLength(1);
  });

  it("stops the login commands on Ctrl+C, as typing does", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["sleep 1"] });
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    setLine("$ ");
    pane.send("\x03");
    opened[0].output("$ ");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["\x03"]);
  });
});
