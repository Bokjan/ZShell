import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";
import "@xterm/xterm/css/xterm.css";

import {
  errorMessage,
  openSession,
  type CommandError,
  type ForwardState,
  type Session,
  type SessionTarget,
} from "../lib/api";
import { isFindShortcut } from "../lib/platform";
import { useSettings } from "../lib/settings";
import { fontStack, resolveScheme, searchDecorations } from "../lib/terminalSchemes";
import { HIGHLIGHT_LIMIT, SearchBar } from "./SearchBar";

export type SessionStatus = "connecting" | "connected" | "closed";

/** Identifies the target, so that the terminal reconnects only when it really changes. */
const targetKey = (target: SessionTarget) => (target.kind === "ssh" ? `ssh:${target.profileId}` : "local");

interface Props {
  target: SessionTarget;
  active: boolean;
  /** Reconnect automatically when an established SSH connection is lost. */
  autoReconnect: boolean;
  onStatus(status: SessionStatus): void;
  /** The shell exited (rather than failing to start or losing the connection). */
  onExited(status: number | null): void;
  /** Reports the backend session id, or null once it has closed. */
  onSession(id: number | null): void;
  onForward(ruleId: string, state: ForwardState): void;
}

/** Seconds to wait before each automatic reconnection attempt; the last one repeats. */
const RETRY_DELAYS = [2, 4, 8, 16, 30];

/** Failures that retrying cannot fix: authentication problems and untrusted host keys. */
const isPermanent = (error: CommandError | null) =>
  !!error && (error.code.startsWith("auth.") || error.code === "ssh.hostKeyRejected");

/** Acknowledge processed output in batches of this many bytes (see `Session.ack`). */
const ACK_BATCH = 64 * 1024;

const ignore = () => {};

export function TerminalView({ target, active, autoReconnect, onStatus, onExited, onSession, onForward }: Props) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const targetRef = useRef(target);
  targetRef.current = target;
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;
  const onExitedRef = useRef(onExited);
  onExitedRef.current = onExited;
  const onSessionRef = useRef(onSession);
  onSessionRef.current = onSession;
  const onForwardRef = useRef(onForward);
  onForwardRef.current = onForward;
  const autoReconnectRef = useRef(autoReconnect);
  autoReconnectRef.current = autoReconnect;
  // Incremented by the find shortcut; 0 means the search bar is closed.
  const [searchKey, setSearchKey] = useState(0);
  const { settings, theme } = useSettings();
  const scheme = resolveScheme(settings.terminal.colorScheme, theme);
  const options = {
    theme: scheme.theme,
    fontFamily: fontStack(settings.terminal.fontFamily),
    fontSize: settings.terminal.fontSize,
    cursorStyle: settings.terminal.cursorStyle,
    cursorBlink: settings.terminal.cursorBlink,
    scrollback: settings.terminal.scrollback,
  };
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    const container = containerRef.current!;
    const term = new Terminal({
      allowProposedApi: true, // required by the unicode11 addon
      ...optionsRef.current,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
    term.loadAddon(search);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon((_event, uri) => void openUrl(uri)));
    term.open(container);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch (e) {
      console.warn("WebGL renderer unavailable, using DOM renderer", e);
    }
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;
    searchRef.current = search;

    const dim = (text: string) => term.write(`\x1b[2m${text}\x1b[0m\r\n`);
    let disposed = false;
    let session: Session | undefined;
    let closed = false;
    // Automatic reconnection: the number of attempts so far and the pending one, if any.
    let attempt = 0;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;

    const cancelRetry = () => {
      clearTimeout(retryTimer);
      retryTimer = undefined;
    };

    const scheduleRetry = () => {
      const delay = RETRY_DELAYS[Math.min(attempt, RETRY_DELAYS.length - 1)];
      attempt++;
      dim(tRef.current("terminal.reconnectIn", { count: delay }));
      retryTimer = setTimeout(() => {
        retryTimer = undefined;
        connect();
      }, delay * 1000);
    };

    const connect = () => {
      cancelRetry();
      closed = false;
      onStatusRef.current("connecting");
      const local = targetRef.current.kind === "local";
      let ended = false;
      let handle: Session | undefined;
      // Output bytes processed by xterm.js and not yet acknowledged.
      let processed = 0;
      const acknowledge = () => {
        if (!handle || processed < ACK_BATCH) return;
        void handle.ack(processed).catch(ignore);
        processed = 0;
      };
      openSession(
        targetRef.current,
        { cols: term.cols, rows: term.rows },
        (data) => {
          if (disposed) return;
          term.write(new Uint8Array(data), () => {
            processed += data.byteLength;
            acknowledge();
          });
        },
        (event) => {
          if (disposed) return;
          if (event.type === "connected") {
            attempt = 0;
            onStatusRef.current("connected");
            return;
          }
          if (event.type === "forward") {
            onForwardRef.current(event.ruleId, event.state);
            return;
          }
          ended = closed = true;
          session = undefined;
          onSessionRef.current(null);
          void handle?.close().catch(ignore);
          onStatusRef.current("closed");
          if (event.reason === "exited") onExitedRef.current(event.status);
          if (local) {
            dim(tRef.current(event.reason === "failed" ? "terminal.retryHint" : "terminal.restartHint"));
            return;
          }
          // Retry lost connections, and keep retrying while the network or server is down.
          const retry =
            autoReconnectRef.current &&
            (event.reason === "lost" || (event.reason === "failed" && attempt > 0)) &&
            !isPermanent(event.error);
          if (retry) scheduleRetry();
          else {
            attempt = 0;
            dim(tRef.current("terminal.reconnectHint"));
          }
        },
      )
        .then((s) => {
          handle = s;
          if (disposed || ended) void s.close().catch(ignore);
          else {
            session = s;
            onSessionRef.current(s.id);
            // Output can arrive before the session id does.
            acknowledge();
          }
        })
        .catch((e) => {
          closed = true;
          attempt = 0;
          onStatusRef.current("closed");
          term.write(`\r\n\x1b[31m${errorMessage(e)}\x1b[0m\r\n`);
          dim(tRef.current("terminal.retryHint"));
        });
    };

    // Don't wait out the delay once the network is back (e.g. after waking from sleep).
    const onOnline = () => retryTimer !== undefined && connect();
    window.addEventListener("online", onOnline);

    const subscriptions: IDisposable[] = [
      term.onData((data) => {
        if (!closed) {
          void session?.write(data);
          return;
        }
        if (data.includes("\r")) connect();
        else if (data.includes("\x03") && retryTimer !== undefined) {
          cancelRetry();
          attempt = 0;
          dim(tRef.current("terminal.reconnectCancelled"));
        }
      }),
      term.onResize(({ cols, rows }) => void session?.resize(cols, rows)),
    ];
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container);
    connect();

    return () => {
      disposed = true;
      cancelRetry();
      window.removeEventListener("online", onOnline);
      observer.disconnect();
      subscriptions.forEach((s) => s.dispose());
      void session?.close().catch(ignore);
      term.dispose();
      termRef.current = fitRef.current = searchRef.current = null;
    };
  }, [targetKey(target)]);

  // Apply appearance and font changes to the running terminal.
  const { theme: termTheme, fontFamily, fontSize, cursorStyle, cursorBlink, scrollback } = options;
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = termTheme;
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    term.options.cursorStyle = cursorStyle;
    term.options.cursorBlink = cursorBlink;
    term.options.scrollback = scrollback;
    fitRef.current?.fit();
  }, [termTheme, fontFamily, fontSize, cursorStyle, cursorBlink, scrollback]);

  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    // Capture phase, so the shortcut never reaches the terminal.
    const onKey = (e: KeyboardEvent) => {
      if (!isFindShortcut(e)) return;
      e.preventDefault();
      e.stopPropagation();
      setSearchKey((key) => key + 1);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [active]);

  const closeSearch = () => {
    setSearchKey(0);
    termRef.current?.focus();
  };

  return (
    // The scheme's background also fills the padding around the terminal.
    <div className="terminal-wrap" style={{ background: scheme.theme.background }}>
      <div className="terminal-view" ref={containerRef} />
      {searchKey > 0 && searchRef.current && (
        <SearchBar
          addon={searchRef.current}
          decorations={searchDecorations(scheme)}
          focusKey={searchKey}
          onClose={closeSearch}
        />
      )}
    </div>
  );
}
