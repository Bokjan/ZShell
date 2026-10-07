import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";
import "@xterm/xterm/css/xterm.css";

import { errorMessage, openSshSession, type ForwardState, type Session } from "../lib/api";

export type SessionStatus = "connecting" | "connected" | "closed";

interface Props {
  profileId: string;
  active: boolean;
  onStatus(status: SessionStatus): void;
  /** Reports the backend session id, or null once it has closed. */
  onSession(id: number | null): void;
  onForward(ruleId: string, state: ForwardState): void;
}

const ignore = () => {};

export function TerminalView({ profileId, active, onStatus, onSession, onForward }: Props) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;
  const onSessionRef = useRef(onSession);
  onSessionRef.current = onSession;
  const onForwardRef = useRef(onForward);
  onForwardRef.current = onForward;

  useEffect(() => {
    const container = containerRef.current!;
    const term = new Terminal({
      allowProposedApi: true, // required by the unicode11 addon
      cursorBlink: true,
      fontFamily: 'Menlo, "Cascadia Mono", Consolas, "PingFang SC", "Microsoft YaHei", monospace',
      fontSize: 13,
      theme: { background: "#1e1e1e" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
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

    let disposed = false;
    let session: Session | undefined;
    let closed = false;

    const connect = () => {
      closed = false;
      onStatusRef.current("connecting");
      let ended = false;
      let handle: Session | undefined;
      openSshSession(
        profileId,
        { cols: term.cols, rows: term.rows },
        (data) => {
          if (!disposed) term.write(new Uint8Array(data));
        },
        (event) => {
          if (disposed) return;
          if (event.type === "connected") {
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
          term.write(`\x1b[2m${tRef.current("terminal.reconnectHint")}\x1b[0m\r\n`);
        },
      )
        .then((s) => {
          handle = s;
          if (disposed || ended) void s.close().catch(ignore);
          else {
            session = s;
            onSessionRef.current(s.id);
          }
        })
        .catch((e) => {
          closed = true;
          onStatusRef.current("closed");
          term.write(`\r\n\x1b[31m${errorMessage(e)}\x1b[0m\r\n\x1b[2m${tRef.current("terminal.retryHint")}\x1b[0m\r\n`);
        });
    };

    const subscriptions: IDisposable[] = [
      term.onData((data) => {
        if (closed) {
          if (data.includes("\r")) connect();
          return;
        }
        void session?.write(data);
      }),
      term.onResize(({ cols, rows }) => void session?.resize(cols, rows)),
    ];
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container);
    connect();

    return () => {
      disposed = true;
      observer.disconnect();
      subscriptions.forEach((s) => s.dispose());
      void session?.close().catch(ignore);
      term.dispose();
      termRef.current = fitRef.current = null;
    };
  }, [profileId]);

  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [active]);

  return <div className="terminal-view" ref={containerRef} />;
}
