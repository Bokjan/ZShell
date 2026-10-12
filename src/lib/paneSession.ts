import type { TFunction } from "i18next";

import {
  adoptSession,
  errorCode,
  errorMessage,
  forwards,
  handOver,
  openSession,
  type CommandError,
  type ForwardState,
  type LogOpen,
  type Session,
  type SessionEvent,
  type SessionId,
  type SessionTarget,
  type ZmodemPhase,
} from "./api";
import type { Arrival, Snapshot } from "./panes";
import { isTerminalReport } from "./terminalReports";
import { TrafficMeter, utf8Length } from "./traffic";

export type SessionStatus = "connecting" | "connected" | "closed";

/** What a pane's session needs from its terminal. */
export interface TerminalPort {
  /** Writes output; `done` runs once the terminal has processed it. */
  write(data: string | Uint8Array, done?: () => void): void;
  size(): { cols: number; rows: number };
  /** The text on the cursor's line before the cursor. */
  lineBeforeCursor(): string;
  /** Whether the alternate screen is shown (a full-screen program). */
  alternateScreen(): boolean;
  /** What the terminal shows, with its scrollback, to draw again elsewhere. */
  snapshot(): Snapshot;
  /** Draws a snapshot (from another window) at its size; `done` runs once it is drawn. */
  restore(snapshot: Snapshot, done: () => void): void;
  /** Fits the terminal to its pane. */
  fit(): void;
}

/** What the session reports as it goes. */
export interface SessionEvents {
  status(status: SessionStatus): void;
  /** The backend session, or null once it has closed. */
  session(id: SessionId | null): void;
  forward(ruleId: string, state: ForwardState): void;
  zmodem(phase: ZmodemPhase): void;
  /** The session's log started (its path) or stopped (null). */
  log(path: string | null): void;
  /** The shell exited (rather than failing to start or losing the connection). */
  exited(status: number | null): void;
  /** An automatic reconnection is due at this time (`Date.now()`), or none is any more (null). */
  retry(at: number | null): void;
  /** The pane that came from another window has started here (see `PaneSession.start`). */
  arrived(): void;
  /** The pane's session couldn't move here from the other window, which still shows it. */
  stayed(): void;
  /** The session moved to another window, which this terminal's screen has been handed to. */
  movedAway(): void;
}

/** Read again for each connection: a saved session may have been changed in between. */
export interface SessionConfig {
  target(): SessionTarget;
  /** How the new session starts its log. */
  logOpen(): LogOpen;
  /**
   * Typed into each new shell, one after another as the shell shows its prompt, until the
   * user types something.
   */
  loginCommands(): string[];
  /** Reconnect automatically when an established connection is lost. */
  autoReconnect(): boolean;
  t: TFunction;
}

/** Seconds to wait before each automatic reconnection attempt; the last one repeats. */
export const RETRY_DELAYS = [2, 4, 8, 16, 30];

/** Proxy failures that retrying cannot fix: it wants credentials we don't have or can't use. */
const PERMANENT_PROXY_ERRORS = new Set(["proxy.authRequired", "proxy.authUnsupported", "proxy.authFailed", "proxy.unsafeName"]);

/** Failures that retrying cannot fix: authentication problems (also with the proxy) and untrusted host keys. */
const isPermanent = (error: CommandError | null) =>
  !!error &&
  (error.code.startsWith("auth.") || error.code === "ssh.hostKeyRejected" || PERMANENT_PROXY_ERRORS.has(error.code));

/** Acknowledge processed output in batches of this many bytes (see `Session.ack`). */
const ACK_BATCH = 64 * 1024;

/** How long the output must pause before the next login command is typed. */
export const LOGIN_COMMAND_IDLE_MS = 300;

/**
 * Whether the text before the cursor looks like a shell prompt: not empty, and not ending
 * like the questions that come before one (passwords and passphrases end with ":", yes/no
 * questions with "?"), so `sudo -i` gets its password before the next command.
 */
const looksLikePrompt = (line: string) => {
  const text = line.trimEnd();
  return text !== "" && !/[:?]$/.test(text);
};

const ignore = () => {};

/**
 * Turns off what a program on the closed connection may have turned on: the alternate
 * screen, mouse reporting, then (soft reset) bracketed paste, application cursor keys, a
 * hidden cursor and so on; otherwise the next shell would get mouse reports as typed text.
 * The backend does this itself when a session ends (`SessionSink::reset_modes`); this is for
 * reconnecting while still connected. Leaving the alternate screen also restores the saved
 * cursor, so only when it is shown.
 */
function resetModes(port: TerminalPort) {
  if (port.alternateScreen()) port.write("\x1b[?1049l");
  port.write("\x1b[?1000l\x1b[?1006l\x1b[!p");
}

/**
 * A pane's backend sessions, one after another: it connects, reconnects by hand or
 * automatically (with growing delays, sooner when the network comes back), types the login
 * commands, and passes output to the terminal with flow control. Enter in a closed terminal
 * connects again; Ctrl+C cancels a pending retry.
 */
export class PaneSession {
  private session: Session | undefined;
  private closed = false;
  /** Whether the current session's shell is up (past its connection's prompts). */
  private connected = false;
  private disposed = false;
  /** Automatic reconnection: the number of attempts so far and the pending one, if any. */
  private attempt = 0;
  private retryTimer: ReturnType<typeof setTimeout> | undefined;
  /**
   * Counts connection attempts; callbacks of an abandoned attempt (see `reconnect`) see a
   * newer value and ignore what arrives.
   */
  private generation = 0;
  /** Forwarding rules that ran before reconnecting by hand, until the new connection is up. */
  private carry: string[] = [];
  /** The login commands not yet typed into the current shell, and the wait for its prompt. */
  private loginPending: string[] = [];
  private loginTimer: ReturnType<typeof setTimeout> | undefined;
  private arriving = false;
  /** What the terminal has sent and received on the current connection. */
  readonly traffic = new TrafficMeter();

  constructor(
    private readonly port: TerminalPort,
    private readonly config: SessionConfig,
    private readonly events: SessionEvents,
    /** Used for the first attempt only: reconnecting always makes a new connection. */
    private shareFrom?: SessionId,
    /** For a pane that came from another window: how it goes on (see `start`). */
    private arrival?: Arrival,
    private readonly open: typeof openSession = openSession,
    private readonly adoptOpen: typeof adoptSession = adoptSession,
  ) {
    window.addEventListener("online", this.onOnline);
  }

  /** The current backend session, while there is one. */
  get id(): SessionId | null {
    return this.session?.id ?? null;
  }

  /** Whether there is no session to type into (it ended, failed or is being replaced). */
  get isClosed() {
    return this.closed;
  }

  /**
   * Whether the session is still moving here from another window: until the screen it showed
   * there is drawn, it can't move on.
   */
  get isArriving() {
    return this.arriving;
  }

  /** Whether the shell is up: connected, past the prompts of connecting (passwords, host keys). */
  get isConnected() {
    return this.connected && !this.closed && this.session !== undefined;
  }

  /**
   * Text from elsewhere, as if typed: the compose bar, a quick command, typing synced from
   * another pane. Only into a shell that is up: never into the prompts of connecting (a
   * passphrase typed in one pane must not reach another), and never reconnecting a closed
   * terminal. Returns whether it was sent.
   */
  send(data: string): boolean {
    return this.isConnected && this.input(data);
  }

  /**
   * What the user typed. Returns whether a session got it. Into a closed terminal, Enter
   * connects again and Ctrl+C cancels a pending retry.
   */
  input(data: string): boolean {
    if (!this.closed) {
      // The user has taken over: a command typed now would land in what they are typing.
      // The terminal's answers to the shell's queries (fish asks at startup) aren't typing.
      if (!isTerminalReport(data)) this.stopLoginCommands();
      if (!this.session) return false;
      void this.session.write(data);
      this.traffic.addSent(utf8Length(data));
      return true;
    }
    // Enter pressed, not a line break in something else that came in (a paste).
    if (data === "\r") this.connect();
    else if (data === "\x03" && this.retryTimer !== undefined) {
      this.cancelRetry();
      this.attempt = 0;
      this.dim(this.config.t("terminal.reconnectCancelled"));
    }
    return false;
  }

  /**
   * What the terminal sends as binary rather than text (mouse reports in the X10 encoding
   * past column 95): one character per byte. Only to a session that is up.
   */
  inputBinary(data: string) {
    if (this.closed || !this.session) return;
    void this.session.writeBytes(Uint8Array.from(data, (c) => c.charCodeAt(0) & 0xff));
    this.traffic.addSent(data.length);
  }

  /** The terminal's new size. Not while a screen from another window is drawn: at its size. */
  resize(cols: number, rows: number) {
    if (this.arriving) return;
    void this.session?.resize(cols, rows);
  }

  /** Closes the current session and connects again; forwarding rules running on it follow. */
  reconnect() {
    const old = this.session;
    this.session = undefined;
    this.connected = false;
    this.events.session(null);
    this.attempt = 0;
    resetModes(this.port);
    this.port.write("\r\n");
    // Leaves the old session's callbacks behind (see `connect`), and any pending retry.
    const current = ++this.generation;
    this.cancelRetry();
    void (async () => {
      if (old) {
        // Added to what an earlier reconnection is still to carry (it never connected).
        const running = await forwards.carry(old.id).catch(() => []);
        this.carry = [...new Set([...this.carry, ...running])];
        // Closed first: while the old connection is still listed, the new one would leave
        // the rules to it (they run there), and they would stop with it.
        await old.close().catch(ignore);
      }
      if (!this.disposed && current === this.generation) this.connect();
    })();
  }

  dispose() {
    this.disposed = true;
    this.cancelRetry();
    this.stopLoginCommands();
    window.removeEventListener("online", this.onOnline);
    void this.session?.close().catch(ignore);
  }

  /**
   * Connects; a pane that came from another window goes on instead: with its session, or
   * showing what it showed there, connecting at once if it was waiting to reconnect.
   */
  start() {
    const arrival = this.arrival;
    this.arrival = undefined;
    if (!arrival) {
      this.connect();
      return;
    }
    this.events.arrived();
    if (arrival.sessionId !== null) this.adopt(arrival.sessionId, arrival.connected);
    else {
      this.closed = true;
      if (arrival.snapshot) this.port.restore(arrival.snapshot, () => this.port.fit());
      if (arrival.reconnect) this.connect();
    }
  }

  connect() {
    this.cancelRetry();
    this.stopLoginCommands();
    this.closed = false;
    this.connected = false;
    this.traffic.reset();
    this.events.status("connecting");
    const target = this.config.target();
    const source = this.shareFrom;
    this.shareFrom = undefined;
    const carry = this.carry;
    this.follow(target, source, false, (onOutput, onEvent) =>
      this.open(target, this.port.size(), onOutput, onEvent, source, this.config.logOpen(), carry),
    );
  }

  /**
   * Takes over session `id` from the window this pane moved from: its screen is drawn first,
   * then the output that came since; the session gets this terminal's size. The session is
   * this pane's as soon as it has moved (typing reaches it, closing the pane ends it). If it
   * can't move, the other window keeps it, and this pane reports `stayed`.
   */
  private adopt(id: SessionId, connected: boolean) {
    this.closed = false;
    this.connected = connected;
    this.arriving = true;
    const settled = () => (this.arriving = false);
    this.follow(
      this.config.target(),
      undefined,
      connected,
      async (onOutput, onEvent, took) => {
        let waiting: (() => void)[] | null = [];
        const later = (f: () => void) => (waiting ? waiting.push(f) : f());
        const adopted = await this.adoptOpen(
          id,
          (data) => later(() => onOutput(data)),
          (event) => later(() => onEvent(event)),
        );
        took(adopted.session);
        const snapshot = await adopted.snapshot;
        if (snapshot) await new Promise<void>((done) => this.port.restore(snapshot, done));
        const pending = waiting;
        waiting = null;
        pending.forEach((f) => f());
        // The output that came meanwhile was written for the size the program knew, the
        // other window's: drawn at it too, and only then fitted to this pane.
        await new Promise<void>((done) => this.port.write("", done));
        settled();
        const before = this.port.size();
        this.port.fit();
        const { cols, rows } = this.port.size();
        // A new size reaches the session as the terminal resizes. Without a screen handed
        // over, the terminal may never have had the session's size: told it here.
        if (cols === before.cols && rows === before.rows) void adopted.session.resize(cols, rows).catch(ignore);
        return adopted.session;
      },
      () => {
        settled();
        this.closed = true;
        this.connected = false;
        this.events.stayed();
      },
    );
  }

  /**
   * Follows a session as `start` opens it: passes its output to the terminal with flow
   * control, and acts on its events. `wasConnected`: its shell is already up (adopted).
   * `start` may say which session it is before it resolves (`took`). `failed`: what to do
   * instead of reporting an error if `start` fails.
   */
  private follow(
    target: SessionTarget,
    source: SessionId | undefined,
    wasConnected: boolean,
    start: (
      onOutput: (data: Uint8Array) => void,
      onEvent: (event: SessionEvent) => void,
      took: (session: Session) => void,
    ) => Promise<Session>,
    failed?: () => void,
  ) {
    const current = ++this.generation;
    const stale = () => this.disposed || current !== this.generation;
    let ended = false;
    let connected = wasConnected;
    let handle: Session | undefined;
    // The session is another window's now: never closed from here.
    let given = false;
    // Output bytes processed by the terminal and not yet acknowledged.
    let processed = 0;
    const acknowledge = () => {
      if (!handle || processed < ACK_BATCH) return;
      void handle.ack(processed).catch(ignore);
      processed = 0;
    };
    const { t } = this.config;
    const took = (s: Session) => {
      if (handle) return;
      handle = s;
      if (stale() || ended) {
        if (!given) void s.close().catch(ignore);
        return;
      }
      this.session = s;
      this.events.session(s.id);
      // Output can arrive before the session id does.
      acknowledge();
      this.awaitPrompt();
    };
    start(
      (data) => {
        if (stale()) return;
        this.traffic.addReceived(data.byteLength);
        this.port.write(data, () => {
          processed += data.byteLength;
          acknowledge();
          this.awaitPrompt();
        });
      },
      (event) => {
        if (stale()) return;
        switch (event.type) {
          case "connected":
            connected = true;
            this.connected = true;
            this.attempt = 0;
            this.carry = [];
            this.events.status("connected");
            this.loginPending = [...this.config.loginCommands()];
            this.awaitPrompt();
            return;
          case "forward":
            this.events.forward(event.ruleId, event.state);
            return;
          case "zmodem":
            this.events.zmodem(event.phase);
            return;
          case "log":
            // A log that stopped midway says so itself; one that couldn't start, with ours.
            if (event.error?.code === "log.writeFailed") this.dim(event.error.message);
            else if (event.error) this.dim(t("terminal.logFailed", { message: event.error.message }));
            this.events.log(event.path);
            return;
          case "moved":
            // The last message: the session is now another window's.
            given = true;
            if (handle) this.moveAway(handle.id, event.moveId);
            return;
        }
        ended = true;
        this.dropSession();
        void handle?.close().catch(ignore);
        // The duplicated tab's connection had died without its session noticing yet (a
        // laptop waking up, say): connect as usual.
        if (source !== undefined && !connected && event.reason === "failed") {
          this.connect();
          return;
        }
        this.events.status("closed");
        if (event.reason === "exited") this.events.exited(event.status);
        if (target.kind === "local") {
          this.dim(t(event.reason === "failed" ? "terminal.retryHint" : "terminal.restartHint"));
          return;
        }
        // Retry lost connections, and keep retrying while the network or server is down.
        const retry =
          this.config.autoReconnect() &&
          (event.reason === "lost" || (event.reason === "failed" && this.attempt > 0)) &&
          !isPermanent(event.error);
        if (retry) this.scheduleRetry();
        else {
          this.attempt = 0;
          this.dim(t("terminal.reconnectHint"));
        }
      },
      took,
    )
      .then(took)
      .catch((e) => {
        // Even for a pane that has closed since: the other window learns it still has the session.
        if (failed) {
          failed();
          return;
        }
        if (stale()) return;
        // The duplicated tab's connection is gone: connect as usual.
        if (source !== undefined && errorCode(e) === "session.notConnected") {
          this.connect();
          return;
        }
        this.closed = true;
        this.attempt = 0;
        this.events.status("closed");
        this.port.write(`\r\n\x1b[31m${errorMessage(e)}\x1b[0m\r\n`);
        this.dim(t("terminal.retryHint"));
      });
  }

  /**
   * The session moved to another window: it is no longer this pane's (closing the pane
   * doesn't end it). The screen is handed over once the terminal has drawn all the output
   * that came before.
   */
  private moveAway(id: SessionId, moveId: string) {
    ++this.generation;
    this.cancelRetry();
    this.dropSession();
    this.port.write("", () => {
      void handOver({ id, moveId, ...this.port.snapshot() }).catch(ignore);
      this.events.movedAway();
    });
  }

  /** Forgets the current session, which has ended or is being replaced. */
  private dropSession() {
    this.closed = true;
    this.connected = false;
    this.stopLoginCommands();
    this.session = undefined;
    this.events.session(null);
  }

  private dim(text: string) {
    this.port.write(`\x1b[2m${text}\x1b[0m\r\n`);
  }

  private stopLoginCommands() {
    this.loginPending = [];
    clearTimeout(this.loginTimer);
  }

  // Called whenever output has been shown: types the next command once the output pauses
  // at something that looks like a prompt.
  private awaitPrompt() {
    if (this.loginPending.length === 0) return;
    clearTimeout(this.loginTimer);
    this.loginTimer = setTimeout(() => {
      if (!this.session || this.closed || !looksLikePrompt(this.port.lineBeforeCursor())) return;
      const command = `${this.loginPending.shift()}\r`;
      void this.session.write(command).catch(ignore);
      this.traffic.addSent(utf8Length(command));
    }, LOGIN_COMMAND_IDLE_MS);
  }

  private cancelRetry() {
    if (this.retryTimer === undefined) return;
    clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
    this.events.retry(null);
  }

  private scheduleRetry() {
    const delay = RETRY_DELAYS[Math.min(this.attempt, RETRY_DELAYS.length - 1)];
    this.attempt++;
    this.dim(this.config.t("terminal.reconnectIn", { count: delay }));
    this.events.retry(Date.now() + delay * 1000);
    this.retryTimer = setTimeout(() => {
      this.retryTimer = undefined;
      this.events.retry(null);
      this.connect();
    }, delay * 1000);
  }

  // Don't wait out the delay once the network is back (e.g. after waking from sleep).
  private readonly onOnline = () => {
    if (this.retryTimer !== undefined) this.connect();
  };
}
