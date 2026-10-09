import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import "@xterm/xterm/css/xterm.css";

import {
  errorCode,
  errorMessage,
  openSession,
  type CommandError,
  type ForwardState,
  type LogOpen,
  type ProfileAppearance,
  type Session,
  type SessionId,
  type SessionTarget,
  type ZmodemPhase,
  zmodem,
} from "../lib/api";
import {
  clipboardKey,
  copyShortcutLabel,
  findShortcutLabel,
  isFindShortcut,
  isMac,
  pasteShortcutLabel,
  selectAllShortcutLabel,
} from "../lib/platform";
import { useSettings } from "../lib/settings";
import { fontStack, searchDecorations, sessionScheme } from "../lib/terminalSchemes";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { HIGHLIGHT_LIMIT, SearchBar } from "./SearchBar";
import { ZmodemBar } from "./ZmodemBar";

export type SessionStatus = "connecting" | "connected" | "closed";

/**
 * What restarts the terminal when it changes. A quick connection saved as a session keeps
 * its terminal and connection; the next connection uses the session.
 */
const targetKey = (target: SessionTarget) => (target.kind === "local" ? "local" : "remote");

interface Props {
  target: SessionTarget;
  /** Start by opening a shell on this session's connection (a duplicated SSH tab). */
  shareFrom?: SessionId;
  /** Changing it closes the current session and connects again. */
  reconnectKey: number;
  active: boolean;
  /** Reconnect automatically when an established SSH connection is lost. */
  autoReconnect: boolean;
  onStatus(status: SessionStatus): void;
  /** The shell exited (rather than failing to start or losing the connection). */
  onExited(status: number | null): void;
  /** Reports the backend session id, or null once it has closed. */
  onSession(id: number | null): void;
  onForward(ruleId: string, state: ForwardState): void;
  /** The title set by the shell (OSC 0 / 2); empty when it clears it. */
  onTitle(title: string): void;
  /** What the user typed or pasted and the session received (not mouse or focus reports). */
  onInput(data: string): void;
  /** Added to the end of the context menu when it opens (pane actions, quick commands). */
  menuItems(): MenuItem[];
  /** How each new session (connection) starts its log. */
  logOpen: LogOpen;
  /** The session's log started (its path) or stopped (null). */
  onLog(path: string | null): void;
  /** The session's own colors and font, over the settings. */
  appearance?: ProfileAppearance;
  /** Typed into each new shell, one after another as the shell shows its prompt. */
  loginCommands: string[];
}

/** Mouse (SGR, X10) and focus reports the terminal sends for programs; not typed input. */
const REPORT = /^\x1b\[(?:<\d+;\d+;\d+[Mm]|M[\s\S]{3}|I|O)$/;

/** Seconds to wait before each automatic reconnection attempt; the last one repeats. */
const RETRY_DELAYS = [2, 4, 8, 16, 30];

/** Failures that retrying cannot fix: authentication problems and untrusted host keys. */
const isPermanent = (error: CommandError | null) =>
  !!error && (error.code.startsWith("auth.") || error.code === "ssh.hostKeyRejected");

/** Acknowledge processed output in batches of this many bytes (see `Session.ack`). */
const ACK_BATCH = 64 * 1024;

/** Characters of a multi-line paste shown in the confirmation dialog. */
const PASTE_PREVIEW = 4000;

/** How long the output must pause before the next login command is typed. */
const LOGIN_COMMAND_IDLE_MS = 300;

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

const copyText = (text: string) => void writeText(text).catch(console.error);

const lineCount = (text: string) => text.replace(/(\r\n|\r|\n)$/, "").split(/\r\n|\r|\n/).length;

export function TerminalView({
  target,
  shareFrom,
  reconnectKey,
  active,
  autoReconnect,
  onStatus,
  onExited,
  onSession,
  onForward,
  onTitle,
  onInput,
  menuItems: extraMenuItems,
  logOpen,
  onLog,
  appearance,
  loginCommands,
}: Props) {
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
  const onTitleRef = useRef(onTitle);
  onTitleRef.current = onTitle;
  const onInputRef = useRef(onInput);
  onInputRef.current = onInput;
  const logOpenRef = useRef(logOpen);
  logOpenRef.current = logOpen;
  const onLogRef = useRef(onLog);
  onLogRef.current = onLog;
  const shareFromRef = useRef(shareFrom);
  shareFromRef.current = shareFrom;
  const loginCommandsRef = useRef(loginCommands);
  loginCommandsRef.current = loginCommands;
  /** Closes the current session and connects again; set while the terminal exists. */
  const reconnectRef = useRef<() => void>(ignore);
  /** Pastes text, asking first if it would run several commands; set while the terminal exists. */
  const pasteRef = useRef<(text: string) => void>(ignore);
  /** The current backend session, for answering ZMODEM transfers. */
  const sessionIdRef = useRef<SessionId | null>(null);
  const onZmodemRef = useRef<(phase: ZmodemPhase) => void>(ignore);
  const autoReconnectRef = useRef(autoReconnect);
  autoReconnectRef.current = autoReconnect;
  // Incremented by the find shortcut; 0 means the search bar is closed.
  const [searchKey, setSearchKey] = useState(0);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  // Multi-line text waiting for the user to confirm the paste.
  const [pendingPaste, setPendingPaste] = useState<string | null>(null);
  // A ZMODEM transfer waiting for files or running; null when there is none.
  const [zmodemPhase, setZmodemPhase] = useState<Exclude<ZmodemPhase, "idle"> | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const { settings, theme, update } = useSettings();
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const scheme = useMemo(
    () => sessionScheme(settings.terminal.colorScheme, theme, appearance),
    [settings.terminal.colorScheme, theme, appearance?.colorScheme, appearance?.background],
  );
  const options = {
    theme: scheme.theme,
    fontFamily: fontStack(appearance?.fontFamily ?? settings.terminal.fontFamily),
    fontSize: appearance?.fontSize ?? settings.terminal.fontSize,
    cursorStyle: settings.terminal.cursorStyle,
    cursorBlink: settings.terminal.cursorBlink,
    scrollback: settings.terminal.scrollback,
    macOptionIsMeta: settings.terminal.optionAsMeta,
    // Otherwise nothing selects text in programs that use the mouse (vim, tmux) on macOS;
    // elsewhere Shift does.
    macOptionClickForcesSelection: true,
    // Right-click pastes in "paste" mode; in "menu" mode it selects a word on macOS, as in
    // Terminal.app (the xterm.js default).
    rightClickSelectsWord: isMac && settings.terminal.rightClick === "menu",
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
    // Counts connection attempts; callbacks of an abandoned attempt (see `reconnect`) see a
    // newer value and ignore what arrives.
    let generation = 0;
    // Used for the first attempt only: reconnecting always makes a new connection.
    let shareFrom = shareFromRef.current;
    // The login commands not yet typed into the current shell, and the wait for its prompt.
    let loginPending: string[] = [];
    let loginTimer: ReturnType<typeof setTimeout> | undefined;

    const stopLoginCommands = () => {
      loginPending = [];
      clearTimeout(loginTimer);
    };

    // Called whenever output has been shown: types the next command once the output pauses
    // at something that looks like a prompt.
    const awaitPrompt = () => {
      if (loginPending.length === 0) return;
      clearTimeout(loginTimer);
      loginTimer = setTimeout(() => {
        const buffer = term.buffer.active;
        const line = buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(false, 0, buffer.cursorX) ?? "";
        if (!session || closed || !looksLikePrompt(line)) return;
        void session.write(`${loginPending.shift()}\r`).catch(ignore);
      }, LOGIN_COMMAND_IDLE_MS);
    };

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
      stopLoginCommands();
      closed = false;
      onStatusRef.current("connecting");
      const local = targetRef.current.kind === "local";
      const current = ++generation;
      const stale = () => disposed || current !== generation;
      const source = shareFrom;
      shareFrom = undefined;
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
          if (stale()) return;
          term.write(new Uint8Array(data), () => {
            processed += data.byteLength;
            acknowledge();
            awaitPrompt();
          });
        },
        (event) => {
          if (stale()) return;
          if (event.type === "connected") {
            attempt = 0;
            onStatusRef.current("connected");
            loginPending = [...loginCommandsRef.current];
            awaitPrompt();
            return;
          }
          if (event.type === "forward") {
            onForwardRef.current(event.ruleId, event.state);
            return;
          }
          if (event.type === "zmodem") {
            onZmodemRef.current(event.phase);
            return;
          }
          if (event.type === "log") {
            if (event.error) dim(tRef.current("terminal.logFailed", { message: event.error.message }));
            onLogRef.current(event.path);
            return;
          }
          ended = closed = true;
          stopLoginCommands();
          session = undefined;
          sessionIdRef.current = null;
          setZmodemPhase(null);
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
        source,
        logOpenRef.current,
      )
        .then((s) => {
          handle = s;
          if (stale() || ended) void s.close().catch(ignore);
          else {
            session = s;
            sessionIdRef.current = s.id;
            onSessionRef.current(s.id);
            // Output can arrive before the session id does.
            acknowledge();
            awaitPrompt();
          }
        })
        .catch((e) => {
          if (stale()) return;
          // The duplicated tab's connection is gone: connect as usual.
          if (source !== undefined && errorCode(e) === "session.notConnected") {
            connect();
            return;
          }
          closed = true;
          attempt = 0;
          onStatusRef.current("closed");
          term.write(`\r\n\x1b[31m${errorMessage(e)}\x1b[0m\r\n`);
          dim(tRef.current("terminal.retryHint"));
        });
    };

    reconnectRef.current = () => {
      const old = session;
      session = undefined;
      sessionIdRef.current = null;
      setZmodemPhase(null);
      onSessionRef.current(null);
      void old?.close().catch(ignore);
      attempt = 0;
      term.write("\r\n");
      connect();
    };

    // Don't wait out the delay once the network is back (e.g. after waking from sleep).
    const onOnline = () => retryTimer !== undefined && connect();
    window.addEventListener("online", onOnline);

    pasteRef.current = (text: string) => {
      if (!text) return;
      // With bracketed paste the shell inserts the lines without running them.
      const confirm = settingsRef.current.terminal.confirmMultilinePaste && !term.modes.bracketedPasteMode;
      if (confirm && /[\r\n]/.test(text)) setPendingPaste(text);
      else term.paste(text);
    };

    // Ctrl+Shift+C / Ctrl+V and friends outside macOS (see `clipboardKey`).
    term.attachCustomKeyEventHandler((e) => {
      const key = clipboardKey(e);
      if (!key || (key === "copyIfSelected" && !term.hasSelection())) return true;
      if (e.type === "keydown") {
        e.preventDefault();
        if (key === "paste") readText().then(pasteRef.current, ignore);
        else {
          copyText(term.getSelection());
          // So that the next Ctrl+C interrupts again.
          term.clearSelection();
        }
      }
      return false;
    });

    // ⌘V (the native Edit menu), and pastes from the browser's own key handling.
    const onPaste = (e: ClipboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      pasteRef.current(e.clipboardData?.getData("text/plain") ?? "");
    };
    container.addEventListener("paste", onPaste, true);

    // Copy on select: once a mouse selection is finished, wherever the button is released.
    let selecting = false;
    const onMouseDown = (e: MouseEvent) => {
      selecting = e.button === 0;
    };
    const onMouseUp = () => {
      if (!selecting) return;
      selecting = false;
      if (settingsRef.current.terminal.copyOnSelect && term.hasSelection()) copyText(term.getSelection());
    };
    container.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("mouseup", onMouseUp);

    // After xterm.js's own handler (which may select the word under the pointer).
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      // Programs using the mouse get the click; Shift+right-click still opens the menu.
      if (term.modes.mouseTrackingMode !== "none" && !e.shiftKey) return;
      if (settingsRef.current.terminal.rightClick === "paste" && !e.shiftKey) {
        readText().then(pasteRef.current, ignore);
        return;
      }
      setMenu({ x: e.clientX, y: e.clientY });
    };
    container.addEventListener("contextmenu", onContextMenu);

    const subscriptions: IDisposable[] = [
      term.onData((data) => {
        if (!closed) {
          if (data.includes("\x03")) stopLoginCommands();
          void session?.write(data);
          if (session && !REPORT.test(data)) onInputRef.current(data);
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
      term.onTitleChange((title) => onTitleRef.current(title)),
    ];
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container);
    connect();

    return () => {
      disposed = true;
      cancelRetry();
      stopLoginCommands();
      window.removeEventListener("online", onOnline);
      window.removeEventListener("mouseup", onMouseUp);
      container.removeEventListener("paste", onPaste, true);
      container.removeEventListener("mousedown", onMouseDown, true);
      container.removeEventListener("contextmenu", onContextMenu);
      reconnectRef.current = pasteRef.current = ignore;
      observer.disconnect();
      subscriptions.forEach((s) => s.dispose());
      void session?.close().catch(ignore);
      term.dispose();
      termRef.current = fitRef.current = searchRef.current = null;
    };
  }, [targetKey(target)]);

  // Apply appearance and font changes to the running terminal.
  const { theme: termTheme, fontFamily, fontSize, cursorStyle, cursorBlink, scrollback } = options;
  const { macOptionIsMeta, rightClickSelectsWord } = options;
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = termTheme;
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    term.options.cursorStyle = cursorStyle;
    term.options.cursorBlink = cursorBlink;
    term.options.scrollback = scrollback;
    term.options.macOptionIsMeta = macOptionIsMeta;
    term.options.rightClickSelectsWord = rightClickSelectsWord;
    fitRef.current?.fit();
  }, [termTheme, fontFamily, fontSize, cursorStyle, cursorBlink, scrollback, macOptionIsMeta, rightClickSelectsWord]);

  // Skips the initial value: only changes ask for a new connection.
  const initialReconnectKey = useRef(reconnectKey);
  useEffect(() => {
    if (reconnectKey !== initialReconnectKey.current) reconnectRef.current();
  }, [reconnectKey]);

  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    // Capture phase, so the shortcut never reaches the terminal. A dialog (settings) above the
    // terminal has the shortcut to itself.
    const onKey = (e: KeyboardEvent) => {
      if (!isFindShortcut(e) || document.querySelector(".dialog-backdrop")) return;
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

  const focus = () => termRef.current?.focus();

  // ZMODEM: `sz` asks where to save (the Downloads folder unless the settings say to ask),
  // `rz` for files: the picker opens right away, and the bar stays for dropping files,
  // choosing again or cancelling.
  const chooseFiles = () => {
    const id = sessionIdRef.current;
    if (id == null) return;
    void openDialog({ multiple: true, title: t("zmodem.chooseFilesTitle") }).then((picked) => {
      if (picked && picked.length > 0) void zmodem.sendFiles(id, picked).catch(console.error);
      focus();
    });
  };

  const cancelZmodem = () => {
    const id = sessionIdRef.current;
    if (id != null) void zmodem.cancel(id).catch(console.error);
  };

  onZmodemRef.current = (phase) => {
    const id = sessionIdRef.current;
    if (id == null) return;
    setZmodemPhase(phase === "idle" ? null : phase);
    if (phase === "chooseFiles") chooseFiles();
    else if (phase === "chooseDestination") {
      if (!settings.zmodem.askDownloadLocation) void zmodem.saveTo(id, null).catch(console.error);
      else {
        void openDialog({ directory: true, title: t("zmodem.chooseFolderTitle") }).then((dir) => {
          if (typeof dir === "string") void zmodem.saveTo(id, dir).catch(console.error);
          else void zmodem.cancel(id).catch(console.error);
          focus();
        });
      }
    }
  };

  // While `rz` waits, files dropped on this terminal are sent.
  const waitingForFiles = active && zmodemPhase === "chooseFiles";
  useEffect(() => {
    if (!waitingForFiles) return;
    const inside = (pos: { x: number; y: number }) => {
      const rect = wrapRef.current?.getBoundingClientRect();
      const x = pos.x / window.devicePixelRatio;
      const y = pos.y / window.devicePixelRatio;
      return !!rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
    };
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "leave") setDragOver(false);
      else if (payload.type === "enter" || payload.type === "over") setDragOver(inside(payload.position));
      else if (payload.type === "drop") {
        setDragOver(false);
        const id = sessionIdRef.current;
        if (inside(payload.position) && id != null && payload.paths.length > 0) {
          void zmodem.sendFiles(id, payload.paths).catch(console.error);
        }
      }
    });
    return () => {
      setDragOver(false);
      void unlisten.then((f) => f());
    };
  }, [waitingForFiles]);

  const menuItems = (): MenuItem[] => {
    const term = termRef.current!;
    return [
      {
        label: t("terminal.menu.copy"),
        shortcut: copyShortcutLabel,
        disabled: !term.hasSelection(),
        onSelect: () => copyText(term.getSelection()),
      },
      {
        label: t("terminal.menu.paste"),
        shortcut: pasteShortcutLabel,
        onSelect: () => void readText().then(pasteRef.current, ignore),
      },
      { label: t("terminal.menu.selectAll"), shortcut: selectAllShortcutLabel, onSelect: () => term.selectAll() },
      "separator" as const,
      { label: t("terminal.menu.find"), shortcut: findShortcutLabel, onSelect: () => setSearchKey((key) => key + 1) },
      "separator" as const,
      { label: t("terminal.menu.clear"), onSelect: () => term.clear() },
      { label: t("terminal.menu.reset"), onSelect: () => term.reset() },
      ...extraMenuItems(),
    ];
  };

  const confirmPaste = (text: string, dontAskAgain: boolean) => {
    setPendingPaste(null);
    if (dontAskAgain) update({ ...settings, terminal: { ...settings.terminal, confirmMultilinePaste: false } });
    termRef.current?.paste(text);
    focus();
  };

  const cancelPaste = () => {
    setPendingPaste(null);
    focus();
  };

  return (
    // The scheme's background also fills the padding around the terminal.
    <div className="terminal-wrap" ref={wrapRef} style={{ background: scheme.theme.background }}>
      <div className="terminal-view" ref={containerRef} />
      {zmodemPhase && (
        <ZmodemBar phase={zmodemPhase} dragOver={dragOver} onChooseFiles={chooseFiles} onCancel={cancelZmodem} />
      )}
      {menu && termRef.current && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems()}
          onClose={() => {
            setMenu(null);
            focus();
          }}
        />
      )}
      {pendingPaste !== null && (
        <ConfirmDialog
          title={t("paste.title")}
          message={t("paste.message", { count: lineCount(pendingPaste) })}
          confirmLabel={t("paste.confirm")}
          checkboxLabel={t("common.dontAskAgain")}
          onConfirm={(dontAskAgain) => confirmPaste(pendingPaste, dontAskAgain)}
          onCancel={cancelPaste}
        >
          <pre className="paste-preview">
            {pendingPaste.length > PASTE_PREVIEW ? `${pendingPaste.slice(0, PASTE_PREVIEW)}…` : pendingPaste}
          </pre>
        </ConfirmDialog>
      )}
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
