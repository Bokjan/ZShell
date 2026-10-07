import { useEffect, useRef } from "react";
import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";
import "@xterm/xterm/css/xterm.css";

import { openLoopbackSession, type Session } from "../lib/session";

export function TerminalView() {
  const containerRef = useRef<HTMLDivElement>(null);

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
    term.focus();

    let disposed = false;
    let session: Session | undefined;
    openLoopbackSession((data) => {
      if (!disposed) term.write(new Uint8Array(data));
    }).then((s) => {
      if (disposed) {
        void s.close();
        return;
      }
      session = s;
      void s.resize(term.cols, term.rows);
    });

    const subscriptions: IDisposable[] = [
      term.onData((data) => void session?.write(data)),
      term.onResize(({ cols, rows }) => void session?.resize(cols, rows)),
    ];
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container);

    return () => {
      disposed = true;
      observer.disconnect();
      subscriptions.forEach((s) => s.dispose());
      void session?.close();
      term.dispose();
    };
  }, []);

  return <div className="terminal-view" ref={containerRef} />;
}
