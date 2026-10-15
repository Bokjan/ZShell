import type { TFunction } from "i18next";
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from "vitest";

import {
  forwards,
  handOver,
  type adoptSession,
  type CommandError,
  type Session,
  type SessionEvent,
  type SessionSnapshot,
  type SessionTarget,
  type openSession,
} from "./api";
import type { Arrival } from "./panes";
import {
  BELL_MS,
  ECHO_MS,
  LOGIN_COMMAND_IDLE_MS,
  PaneSession,
  type SessionConfig,
  type SessionEvents,
  type TerminalPort,
} from "./paneSession";

// The forwarding rules running on a connection being replaced (see `reconnect`), and the
// screen handed over when a session moves to another window.
vi.mock("./api", async (importOriginal) => {
  const api = await importOriginal<typeof import("./api")>();
  return { ...api, forwards: { ...api.forwards, carry: vi.fn(async () => ["r1"]) }, handOver: vi.fn(async () => {}) };
});

/** One call of the fake `openSession`. */
interface Opened {
  target: SessionTarget;
  shareFrom: number | undefined;
  carry: string[];
  output(text: string): void;
  event(event: SessionEvent): void;
  /** Settles the call: a session, or an error. */
  resolve(): Session & { writes: string[]; acks: number[]; closed: boolean };
  reject(error: unknown): void;
}

const lost = (error: CommandError | null = null): SessionEvent => ({ type: "closed", reason: "lost", error, status: null, signal: null });
const failed = (error: CommandError | null = null): SessionEvent => ({
  type: "closed",
  reason: "failed",
  error,
  status: null,
  signal: null,
});

let nextId = 1;

/** A fake session, recording what it is sent. */
function fakeSession(id = nextId++) {
  const session = {
    id,
    writes: [] as string[],
    acks: [] as number[],
    sizes: [] as [number, number][],
    closed: false,
    write: async (data: string) => void session.writes.push(data),
    writeBytes: async (data: Uint8Array) => void session.writes.push(String.fromCharCode(...data)),
    resize: async (cols: number, rows: number) => void session.sizes.push([cols, rows]),
    ack: async (bytes: number) => void session.acks.push(bytes),
    close: async () => void (session.closed = true),
  };
  return session;
}

/** The one call of the fake `adoptSession`. */
interface Adopted {
  id: number;
  output(text: string): void;
  event(event: SessionEvent): void;
  session: ReturnType<typeof fakeSession>;
  /** The old window hands the screen over (or doesn't: null). */
  handOver(snapshot: SessionSnapshot | null): void;
}

function setup(config: Partial<SessionConfig> = {}, shareFrom?: number, arrival?: Arrival) {
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
          const session = fakeSession();
          resolve(session);
          return session;
        },
        reject,
      });
    })) as typeof openSession;
  let line = "";
  const written: string[] = [];
  // The terminal's size: the pane's (80x24) once fitted. Each write notes the size it was
  // drawn at.
  let size = { cols: 80, rows: 24 };
  const drawnAt: string[] = [];
  const port: TerminalPort = {
    write: (data, done) => {
      const text = typeof data === "string" ? data : new TextDecoder().decode(data);
      if (text) {
        written.push(text);
        drawnAt.push(`${text} ${size.cols}x${size.rows}`);
      }
      done?.();
    },
    size: () => size,
    lineBeforeCursor: () => line,
    alternateScreen: () => false,
    snapshot: () => ({ data: "<screen>", cols: 80, rows: 24 }),
    restore: (snapshot, done) => {
      size = { cols: snapshot.cols, rows: snapshot.rows };
      written.push(`<restored ${snapshot.data} ${snapshot.cols}x${snapshot.rows}>`);
      done();
    },
    // As the terminal does: a new size is passed on.
    fit: () => {
      if (size.cols === 80 && size.rows === 24) return;
      size = { cols: 80, rows: 24 };
      pane.resize(80, 24);
    },
  };
  const adopted: Adopted[] = [];
  let adoptFailure: unknown = null;
  const adopt = (async (id, onOutput, onEvent) => {
    if (adoptFailure) throw adoptFailure;
    const session = fakeSession(id);
    let handOver: (snapshot: SessionSnapshot | null) => void = () => {};
    const snapshot = new Promise<SessionSnapshot | null>((resolve) => (handOver = resolve));
    adopted.push({ id, output: (text) => onOutput(new TextEncoder().encode(text)), event: onEvent, session, handOver });
    return { session, snapshot };
  }) as typeof adoptSession;
  const events = {
    status: vi.fn(),
    session: vi.fn(),
    forward: vi.fn(),
    zmodem: vi.fn(),
    log: vi.fn(),
    exited: vi.fn((_status: number | null, _signal: string | null, _typed: boolean) => false),
    close: vi.fn(),
    retry: vi.fn(),
    arrived: vi.fn(),
    stayed: vi.fn(),
    movedAway: vi.fn(),
    output: vi.fn(),
    bell: vi.fn(),
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
    arrival,
    open,
    adopt,
  );
  return {
    pane,
    opened,
    adopted,
    /** The session can't move here: adopting it fails with `error`. */
    failAdoption: (error: unknown) => (adoptFailure = error),
    events,
    written,
    drawnAt,
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

  it("reports when the next attempt is due, until it starts or is cancelled", async () => {
    const { pane, opened, events } = setup();
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event(lost());
    expect(events.retry).toHaveBeenLastCalledWith(Date.now() + 2000);
    await vi.advanceTimersByTimeAsync(2000);
    expect(events.retry).toHaveBeenLastCalledWith(null);
    opened[1].event({ type: "connected" });
    opened[1].event(lost());
    expect(events.retry).toHaveBeenLastCalledWith(Date.now() + 2000);
    pane.input("\x03");
    expect(events.retry).toHaveBeenLastCalledWith(null);
    expect(events.retry).toHaveBeenCalledTimes(4);
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

  it("connects nothing while moving to another window, and once back if asked to meanwhile", async () => {
    const { pane, opened } = setup();
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event(lost());
    pane.suspend();
    await vi.advanceTimersByTimeAsync(60_000);
    window.dispatchEvent(new Event("online"));
    pane.input("\r");
    expect(opened).toHaveLength(1);
    // The move failed.
    pane.resume();
    expect(opened).toHaveLength(2);
    // Nothing was asked for while it was suspended: resuming connects nothing.
    opened[1].event(failed());
    pane.suspend();
    pane.resume();
    expect(opened).toHaveLength(2);
  });

  it("never retries a local terminal", async () => {
    const { pane, opened, events, screen } = setup({ target: () => ({ kind: "local" }) });
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event({ type: "closed", reason: "exited", error: null, status: 0, signal: null });
    expect(events.exited).toHaveBeenCalledWith(0, null, false);
    expect(screen()).toContain("terminal.restartHint");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(opened).toHaveLength(1);
  });
});

describe("ending", () => {
  const exited = (status: number | null): SessionEvent => ({ type: "closed", reason: "exited", error: null, status, signal: null });

  it("says whether the user typed into the shell, not into the prompts of connecting", async () => {
    const { pane, opened, events } = setup();
    pane.connect();
    opened[0].resolve();
    await settle();
    pane.input("password\r");
    opened[0].event({ type: "connected" });
    // The terminal's answers to the shell's queries aren't typing.
    pane.input("\x1b[?1;2c");
    opened[0].event(exited(1));
    expect(events.exited).toHaveBeenLastCalledWith(1, null, false);

    pane.input("\r");
    opened[1].resolve();
    await settle();
    opened[1].event({ type: "connected" });
    pane.input("exit\r");
    opened[1].event(exited(null));
    expect(events.exited).toHaveBeenLastCalledWith(null, null, true);
  });

  it("shows nothing more when the pane closes", async () => {
    const { pane, opened, events, screen } = setup();
    events.exited.mockReturnValueOnce(true);
    pane.connect();
    opened[0].event({ type: "connected" });
    opened[0].event(exited(0));
    expect(events.status).not.toHaveBeenCalledWith("closed");
    expect(screen()).not.toContain("terminal.reconnectHint");
    expect(pane.isClosed).toBe(true);
  });

  it("asks to close the pane on Ctrl+D once closed, and only then", async () => {
    const { pane, opened, events } = setup();
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    pane.input("\x04");
    await settle();
    expect(session.writes).toEqual(["\x04"]);
    expect(events.close).not.toHaveBeenCalled();
    opened[0].event(lost());
    pane.input("\x04");
    expect(events.close).toHaveBeenCalledOnce();
  });
});

describe("traffic", () => {
  it("counts what the terminal sends and receives on each connection", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["cd /"] });
    pane.connect();
    // Nothing is sent before there is a session to send it to.
    expect(pane.input("x")).toBe(false);
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    opened[0].output("é$ ");
    setLine("$ ");
    await vi.advanceTimersByTimeAsync(1000);
    expect(session.writes).toEqual(["cd /\r"]);
    pane.input("ls\r");
    pane.inputBinary("\x1b[M !!");
    expect(pane.traffic).toMatchObject({ sent: 5 + 3 + 6, received: 4 });
    expect(pane.traffic.lastSent).toBe(Date.now());

    opened[0].event(lost());
    await vi.advanceTimersByTimeAsync(2000);
    expect(pane.traffic).toMatchObject({ sent: 0, received: 0, lastSent: null, lastReceived: null });
  });

  it("reports output, but not what answers typing or resizing", async () => {
    let now = 0;
    const clock = vi.spyOn(performance, "now").mockImplementation(() => now);
    onTestFinished(() => clock.mockRestore());
    const { pane, opened, events } = setup();
    pane.connect();
    opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    opened[0].output("banner");
    expect(events.output).toHaveBeenCalledTimes(1);
    // The echo of what was typed, and a program drawing again at the new size.
    now = 1000;
    pane.input("l");
    now += ECHO_MS - 1;
    opened[0].output("l");
    now = 2000;
    pane.resize(100, 30);
    now += ECHO_MS - 1;
    opened[0].output("redrawn");
    expect(events.output).toHaveBeenCalledTimes(1);
    now += 1;
    opened[0].output("news");
    expect(events.output).toHaveBeenCalledTimes(2);
  });

  it("rings once for bells in quick succession, saying whether they answer typing", async () => {
    let now = 10_000;
    const clock = vi.spyOn(performance, "now").mockImplementation(() => now);
    onTestFinished(() => clock.mockRestore());
    const { pane, opened, events } = setup();
    pane.connect();
    opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    pane.bell();
    now += BELL_MS - 1;
    pane.bell();
    expect(events.bell.mock.calls).toEqual([[false]]);
    now += 1;
    pane.bell();
    expect(events.bell.mock.calls).toEqual([[false], [false]]);
    // A failed completion: the bell answers the Tab key.
    now += BELL_MS;
    pane.input("\t");
    now += ECHO_MS - 1;
    pane.bell();
    expect(events.bell).toHaveBeenLastCalledWith(true);
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

  it("connects again once the old session has closed", async () => {
    const { pane, opened } = setup();
    pane.connect();
    const first = opened[0].resolve();
    await settle();
    let closed = () => {};
    first.close = () => new Promise<void>((resolve) => (closed = resolve));
    pane.reconnect();
    await settle();
    expect(opened).toHaveLength(1);
    closed();
    await settle();
    expect(opened).toHaveLength(2);
  });

  it("starts the forwarding rules that ran on the new connection, once", async () => {
    const { pane, opened } = setup({ target: () => ({ kind: "profile", profileId: "p" }) });
    pane.connect();
    opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    pane.reconnect();
    await settle();
    expect(opened[1].carry).toEqual(["r1"]);
    opened[1].resolve();
    await settle();
    opened[1].event({ type: "connected" });
    // A later automatic reconnection starts the session's automatic rules only.
    opened[1].event(lost());
    await vi.runOnlyPendingTimersAsync();
    expect(opened[2].carry).toEqual([]);
  });

  it("keeps the rules to carry when reconnecting again before connecting", async () => {
    const { pane, opened } = setup({ target: () => ({ kind: "profile", profileId: "p" }) });
    pane.connect();
    opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    pane.reconnect();
    await settle();
    // Waiting at a password prompt: nothing runs on it yet.
    opened[1].resolve();
    await settle();
    vi.mocked(forwards.carry).mockResolvedValueOnce([]);
    pane.reconnect();
    await settle();
    expect(opened[2].carry).toEqual(["r1"]);
  });
});

describe("flow control", () => {
  it("acknowledges what the terminal has processed in batches", async () => {
    const { pane, opened } = setup();
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].output("x".repeat(40_000));
    expect(session.acks).toEqual([]);
    opened[0].output("y".repeat(40_000));
    expect(session.acks).toEqual([80_000]);
  });

  it("acknowledges output that came before the session's id", async () => {
    const { pane, opened } = setup();
    pane.connect();
    opened[0].output("z".repeat(70_000));
    const session = opened[0].resolve();
    await settle();
    expect(session.acks).toEqual([70_000]);
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

  it("stops typing them once the user types", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["cd /srv"] });
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    setLine("$ ");
    pane.input("l");
    setLine("$ l");
    opened[0].output("l");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["l"]);
  });

  it("goes on past the terminal's answers to the shell's queries", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["cd /srv"] });
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    pane.input("\x1b[?62;22c");
    setLine("$ ");
    opened[0].output("$ ");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["\x1b[?62;22c", "cd /srv\r"]);
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

  it("stops the login commands, as typing does", async () => {
    const { pane, opened, setLine } = setup({ loginCommands: () => ["sleep 1"] });
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    setLine("$ ");
    pane.send("ls\r");
    opened[0].output("$ ");
    await vi.advanceTimersByTimeAsync(LOGIN_COMMAND_IDLE_MS);
    expect(session.writes).toEqual(["ls\r"]);
  });
});

describe("moving to another window", () => {
  const from = { window: "main", pane: 1 };

  it("hands the screen over once the session has moved, and leaves the session open", async () => {
    const { pane, opened, events, written } = setup();
    pane.connect();
    const session = opened[0].resolve();
    await settle();
    opened[0].event({ type: "connected" });
    opened[0].output("$ ");
    opened[0].event({ type: "moved", moveId: "m1" });
    expect(handOver).toHaveBeenLastCalledWith({ id: session.id, moveId: "m1", data: "<screen>", cols: 80, rows: 24 });
    expect(events.movedAway).toHaveBeenCalledOnce();
    expect(events.session).toHaveBeenLastCalledWith(null);
    // Nothing more goes to it from here, and closing the pane doesn't end it.
    expect(pane.input("ls\r")).toBe(false);
    pane.dispose();
    await settle();
    expect(session.closed).toBe(false);
    expect(written.join("")).toBe("$ ");
  });

  it("takes the session over: its screen first, then the output that came since", async () => {
    const arrival: Arrival = { sessionId: 42, connected: true, snapshot: null, reconnect: false, from };
    const { pane, adopted, events, screen, opened, drawnAt } = setup({}, undefined, arrival);
    pane.start();
    expect(events.arrived).toHaveBeenCalledOnce();
    await settle();
    expect(adopted[0].id).toBe(42);
    adopted[0].output("late");
    expect(screen()).toBe("");
    // It is this pane's already: typing reaches it, while it can't move on yet.
    expect(events.session).toHaveBeenLastCalledWith(42);
    expect(pane.isArriving).toBe(true);
    expect(pane.send("ls\r")).toBe(true);
    // The terminal resizing meanwhile isn't passed on.
    pane.resize(90, 20);
    adopted[0].handOver({ id: 42, moveId: "m1", data: "<old>", cols: 100, rows: 30 });
    await settle();
    expect(screen()).toBe("<restored <old> 100x30>late");
    // Drawn at the size the program wrote it for; then fitted, and the session told once.
    expect(drawnAt).toEqual(["late 100x30"]);
    expect(pane.isArriving).toBe(false);
    expect(adopted[0].session.sizes).toEqual([[80, 24]]);
    // Its shell was up: nothing connects anew.
    expect(adopted[0].session.writes).toEqual(["ls\r"]);
    expect(opened).toHaveLength(0);
  });

  it("moves on without closing the session it took over", async () => {
    const { pane, adopted, events } = setup({}, undefined, { sessionId: 42, connected: true, snapshot: null, reconnect: false, from });
    pane.start();
    await settle();
    adopted[0].handOver(null);
    await settle();
    adopted[0].event({ type: "moved", moveId: "m2" });
    expect(handOver).toHaveBeenLastCalledWith({ id: 42, moveId: "m2", data: "<screen>", cols: 80, rows: 24 });
    expect(events.movedAway).toHaveBeenCalledOnce();
    pane.dispose();
    await settle();
    expect(adopted[0].session.closed).toBe(false);
  });

  it("reports a session that couldn't move here, without an error or connecting (the pane closes)", async () => {
    const { pane, failAdoption, events, screen, opened } = setup({}, undefined, {
      sessionId: 42,
      connected: true,
      snapshot: null,
      reconnect: false,
      from,
    });
    failAdoption({ code: "session.busy", message: "busy" });
    pane.start();
    await settle();
    expect(events.stayed).toHaveBeenCalledOnce();
    expect(pane.isArriving).toBe(false);
    expect(screen()).toBe("");
    expect(opened).toHaveLength(0);
  });

  it("goes on without the screen if the old window doesn't hand it over", async () => {
    const arrival: Arrival = { sessionId: 7, connected: true, snapshot: null, reconnect: false, from };
    const { pane, adopted, screen } = setup({}, undefined, arrival);
    pane.start();
    await settle();
    adopted[0].output("late");
    adopted[0].handOver(null);
    await settle();
    expect(screen()).toBe("late");
  });

  it("shows what a pane without a session showed, and connects on Enter", async () => {
    const snapshot = { data: "<bye>", cols: 80, rows: 24 };
    const { pane, opened, screen } = setup({}, undefined, { sessionId: null, connected: false, snapshot, reconnect: false, from });
    pane.start();
    expect(screen()).toBe("<restored <bye> 80x24>");
    expect(pane.isClosed).toBe(true);
    expect(opened).toHaveLength(0);
    pane.input("\r");
    expect(opened).toHaveLength(1);
  });

  it("connects at once if it was waiting to reconnect", () => {
    const { pane, opened } = setup({}, undefined, { sessionId: null, connected: false, snapshot: null, reconnect: true, from });
    pane.start();
    expect(opened).toHaveLength(1);
  });
});
