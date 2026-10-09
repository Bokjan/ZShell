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

import { type ProfileAppearance, type ZmodemPhase, zmodem } from "../lib/api";
import type { PaneSession } from "../lib/paneSession";
import {
  clipboardKey,
  copyShortcutLabel,
  findShortcutLabel,
  isFindShortcut,
  isMac,
  pasteShortcutLabel,
  selectAllShortcutLabel,
} from "../lib/platform";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { useSettings } from "../lib/settings";
import { useShortcuts } from "../lib/shortcuts";
import { fontStack, searchDecorations, sessionScheme } from "../lib/terminalSchemes";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { HIGHLIGHT_LIMIT, SearchBar } from "./SearchBar";
import { ZmodemBar } from "./ZmodemBar";

interface Props {
  /** The pane whose terminal this is; its session is in `sessions`. */
  paneKey: number;
  sessions: SessionRegistry;
  /** Whether the pane's tab is shown; hidden terminals give up their WebGL renderer. */
  visible: boolean;
  /** Shown and focused: it has the keyboard. */
  active: boolean;
  /** The title set by the shell (OSC 0 / 2); empty when it clears it. */
  onTitle(title: string): void;
  /** What the user typed and the session received (not mouse or focus reports, nor pastes). */
  onInput(data: string): void;
  /** Lets other panes paste into this one (while syncing), or stops (null). */
  registerPaste(target: PasteTarget | null): void;
  /** The other panes a paste goes to as well (syncing): each pastes it its own way. */
  pasteTargets(): PasteTarget[];
  /** Added to the end of the context menu when it opens (pane actions, quick commands). */
  menuItems(): MenuItem[];
  /** The session's own colors and font, over the settings. */
  appearance?: ProfileAppearance;
}

/** A pane that text can be pasted into, as its terminal's mode requires. */
export interface PasteTarget {
  /** Whether the program in it takes bracketed pastes (inserting lines without running them). */
  bracketed(): boolean;
  paste(text: string): void;
}

/**
 * What the terminal sends for programs rather than for the user, so not synced to other
 * panes: mouse (SGR, X10) and focus reports, and answers to queries (device attributes,
 * cursor position, status, modes, window size, OSC colors, DCS). Each comes as one `onData`.
 * A cursor position report can't be told apart from Shift+F3 (`ESC[1;2R`), which is not
 * synced either.
 */
const REPORT =
  /^\x1b(?:\[(?:<\d+;\d+;\d+[Mm]|M[\s\S]{3}|I|O|[?>]?[\d;]*c|\??\d+;\d+(?:\$y|R)|\d*n|[\d;]*t)|[\]P][\s\S]*(?:\x1b\\|\x07))$/;

/** Characters of a multi-line paste shown in the confirmation dialog. */
const PASTE_PREVIEW = 4000;

const ignore = () => {};

const copyText = (text: string) => void writeText(text).catch(console.error);

const lineCount = (text: string) => text.replace(/(\r\n|\r|\n)$/, "").split(/\r\n|\r|\n/).length;

export function TerminalView({
  paneKey,
  sessions,
  visible,
  active,
  onTitle,
  onInput,
  registerPaste,
  pasteTargets,
  menuItems: extraMenuItems,
  appearance,
}: Props) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const onTitleRef = useRef(onTitle);
  onTitleRef.current = onTitle;
  const onInputRef = useRef(onInput);
  onInputRef.current = onInput;
  const registerPasteRef = useRef(registerPaste);
  registerPasteRef.current = registerPaste;
  const pasteTargetsRef = useRef(pasteTargets);
  pasteTargetsRef.current = pasteTargets;
  /** Pastes text, asking first if it would run several commands; set while the terminal exists. */
  const pasteRef = useRef<(text: string) => void>(ignore);
  /** Pastes into this pane and the panes synced with it, without asking. */
  const pasteAllRef = useRef<(text: string) => void>(ignore);
  /** The pane's session, for answering ZMODEM transfers. */
  const sessionRef = useRef<PaneSession | null>(null);
  const webglRef = useRef<WebglAddon | null>(null);
  const onZmodemRef = useRef<(phase: ZmodemPhase) => void>(ignore);
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
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;
    searchRef.current = search;

    const session = sessionsRef.current.attach(paneKey, {
      port: {
        write: (data, done) => term.write(data, done),
        size: () => ({ cols: term.cols, rows: term.rows }),
        lineBeforeCursor: () => {
          const buffer = term.buffer.active;
          return buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(false, 0, buffer.cursorX) ?? "";
        },
        alternateScreen: () => term.buffer.active.type === "alternate",
      },
      zmodem: (phase) => onZmodemRef.current(phase),
      session: () => setZmodemPhase(null),
    });
    sessionRef.current = session;

    // While syncing, a paste isn't passed on as typed input (wrapped for bracketed paste, or
    // not, as this terminal is): each synced pane pastes it as its own terminal requires.
    let pasting = false;
    pasteAllRef.current = (text: string) => {
      const others = pasteTargetsRef.current();
      pasting = true;
      try {
        term.paste(text);
      } finally {
        pasting = false;
      }
      for (const other of others) other.paste(text);
    };
    pasteRef.current = (text: string) => {
      // Nowhere to go; a line break in it would otherwise read as Enter, which reconnects.
      if (!text || session.isClosed) return;
      // With bracketed paste the shell inserts the lines without running them: asked unless
      // every pane it goes to does.
      const bracketed = term.modes.bracketedPasteMode && pasteTargetsRef.current().every((other) => other.bracketed());
      const confirm = settingsRef.current.terminal.confirmMultilinePaste && !bracketed;
      if (confirm && /[\r\n]/.test(text)) setPendingPaste(text);
      else pasteAllRef.current(text);
    };
    registerPasteRef.current({
      bracketed: () => term.modes.bracketedPasteMode,
      paste: (text) => {
        if (text && !session.isClosed) term.paste(text);
      },
    });

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
        if (session.input(data) && !pasting && !REPORT.test(data)) onInputRef.current(data);
      }),
      term.onResize(({ cols, rows }) => session.resize(cols, rows)),
      term.onTitleChange((title) => onTitleRef.current(title)),
    ];
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(container);
    // A tick later: React's StrictMode (in development) mounts, unmounts and mounts again at
    // once, which would open a backend session (and a log file, or a serial device that is
    // then busy) for the first mount too.
    const start = setTimeout(() => session.connect(), 0);

    return () => {
      clearTimeout(start);
      window.removeEventListener("mouseup", onMouseUp);
      container.removeEventListener("paste", onPaste, true);
      container.removeEventListener("mousedown", onMouseDown, true);
      container.removeEventListener("contextmenu", onContextMenu);
      pasteRef.current = pasteAllRef.current = ignore;
      registerPasteRef.current(null);
      observer.disconnect();
      subscriptions.forEach((s) => s.dispose());
      sessionsRef.current.detach(paneKey, session);
      sessionRef.current = null;
      term.dispose();
      termRef.current = fitRef.current = searchRef.current = null;
    };
  }, [paneKey]);

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

  // The web view allows only so many WebGL contexts (about 16), and drops the oldest beyond
  // that: terminals of hidden tabs release theirs (drawing with the DOM renderer meanwhile)
  // and create one again when shown. A lost context is replaced the next time too.
  useEffect(() => {
    const term = termRef.current;
    if (!term || !visible) return;
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        webgl.dispose();
        if (webglRef.current === webgl) webglRef.current = null;
      });
      term.loadAddon(webgl);
      webglRef.current = webgl;
    } catch (e) {
      console.warn("WebGL renderer unavailable, using DOM renderer", e);
    }
    return () => {
      // A disposed terminal (unmounting) has disposed its addons itself.
      if (termRef.current) webglRef.current?.dispose();
      webglRef.current = null;
    };
  }, [visible]);

  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [active]);

  // A dialog (settings) above the terminal has the shortcut to itself.
  useShortcuts(
    (e) => {
      if (!isFindShortcut(e)) return false;
      setSearchKey((key) => key + 1);
      return true;
    },
    { enabled: active },
  );

  const closeSearch = () => {
    setSearchKey(0);
    termRef.current?.focus();
  };

  const focus = () => termRef.current?.focus();

  // ZMODEM: `sz` asks where to save: the bar asks (download folder, another folder or
  // cancel) unless the settings say to use the download folder, or to open the folder picker
  // right away. `rz` asks for files: the picker opens right away. Either way the bar stays,
  // for choosing again (or dropping files) or cancelling, after the picker is closed.
  const chooseFiles = () => {
    const id = sessionRef.current?.id;
    if (id == null) return;
    void openDialog({ multiple: true, title: t("zmodem.chooseFilesTitle") }).then((picked) => {
      if (picked && picked.length > 0) void zmodem.sendFiles(id, picked).catch(console.error);
      focus();
    });
  };

  const saveReceived = (dir: string | null) => {
    const id = sessionRef.current?.id;
    if (id != null) void zmodem.saveTo(id, dir).catch(console.error);
  };

  const chooseFolder = () => {
    void openDialog({ directory: true, title: t("zmodem.chooseFolderTitle") }).then((dir) => {
      if (typeof dir === "string") saveReceived(dir);
      focus();
    });
  };

  const cancelZmodem = () => {
    const id = sessionRef.current?.id;
    if (id != null) void zmodem.cancel(id).catch(console.error);
  };

  onZmodemRef.current = (phase) => {
    const id = sessionRef.current?.id;
    if (id == null) return;
    setZmodemPhase(phase === "idle" ? null : phase);
    if (phase === "chooseFiles") chooseFiles();
    else if (phase === "chooseDestination") {
      if (settings.zmodem.receive === "downloads") saveReceived(null);
      else if (settings.zmodem.receive === "chooseFolder") chooseFolder();
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
        const id = sessionRef.current?.id;
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
    pasteAllRef.current(text);
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
        <ZmodemBar
          phase={zmodemPhase}
          dragOver={dragOver}
          onChooseFiles={chooseFiles}
          onSaveToDownloads={() => saveReceived(null)}
          onChooseFolder={chooseFolder}
          onCancel={cancelZmodem}
        />
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
