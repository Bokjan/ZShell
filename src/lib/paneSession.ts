import type { TFunction } from "i18next";

import {
  errorCode,
  errorMessage,
  forwards,
  openSession,
  type CommandError,
  type ForwardState,
  type LogOpen,
  type Session,
  type SessionId,
  type SessionTarget,
  type ZmodemPhase,
} from "./api";

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
}

/** Read again for each connection: a saved session may have been changed in between. */
export interface SessionConfig {
  target(): SessionTarget;
  /** How the new session starts its log. */
  logOpen(): LogOpen;
  /** Typed into each new shell, one after another as the shell shows its prompt. */
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

  constructor(
    private readonly port: TerminalPort,
    private readonly config: SessionConfig,
    private readonly events: SessionEvents,
    /** Used for the first attempt only: reconnecting always makes a new connection. */
    private shareFrom?: SessionId,
    private readonly open: typeof openSession = openSession,
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
      if (data.includes("\x03")) this.stopLoginCommands();
      void this.session?.write(data);
      return this.session !== undefined;
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
    if (this.closed) return;
    void this.session?.writeBytes(Uint8Array.from(data, (c) => c.charCodeAt(0) & 0xff));
  }

  resize(cols: number, rows: number) {
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
        void old.close().catch(ignore);
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

  connect() {
    this.cancelRetry();
    this.stopLoginCommands();
    this.closed = false;
    this.connected = false;
    this.events.status("connecting");
    const target = this.config.target();
    const current = ++this.generation;
    const stale = () => this.disposed || current !== this.generation;
    const source = this.shareFrom;
    this.shareFrom = undefined;
    let ended = false;
    let connected = false;
    let handle: Session | undefined;
    // Output bytes processed by the terminal and not yet acknowledged.
    let processed = 0;
    const acknowledge = () => {
      if (!handle || processed < ACK_BATCH) return;
      void handle.ack(processed).catch(ignore);
      processed = 0;
    };
    const { t } = this.config;
    this.open(
      target,
      this.port.size(),
      (data) => {
        if (stale()) return;
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
      source,
      this.config.logOpen(),
      this.carry,
    )
      .then((s) => {
        handle = s;
        if (stale() || ended) void s.close().catch(ignore);
        else {
          this.session = s;
          this.events.session(s.id);
          // Output can arrive before the session id does.
          acknowledge();
          this.awaitPrompt();
        }
      })
      .catch((e) => {
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
      void this.session.write(`${this.loginPending.shift()}\r`).catch(ignore);
    }, LOGIN_COMMAND_IDLE_MS);
  }

  private cancelRetry() {
    clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
  }

  private scheduleRetry() {
    const delay = RETRY_DELAYS[Math.min(this.attempt, RETRY_DELAYS.length - 1)];
    this.attempt++;
    this.dim(this.config.t("terminal.reconnectIn", { count: delay }));
    this.retryTimer = setTimeout(() => {
      this.retryTimer = undefined;
      this.connect();
    }, delay * 1000);
  }

  // Don't wait out the delay once the network is back (e.g. after waking from sleep).
  private readonly onOnline = () => {
    if (this.retryTimer !== undefined) this.connect();
  };
}
