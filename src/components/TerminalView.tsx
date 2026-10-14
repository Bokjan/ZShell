import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import "@xterm/xterm/css/xterm.css";

import { currentCjk } from "../i18n";
import { type ProfileAppearance, type ZmodemPhase, zmodem } from "../lib/api";
import { isAppLocked, whenUnlocked } from "../lib/appLock";
import { afterDialogs, isDialogOpen } from "../lib/dialogs";
import { open as openDialog } from "../lib/fileDialog";
import { chosenAction, clipboardKeyOf, matches, shortcutLabel } from "../lib/keymap";
import { followsMenuKey, isMenuKey, openedByMenuKey } from "../lib/menuKey";
import type { PaneSession } from "../lib/paneSession";
import { isMac } from "../lib/platform";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { useMediaQuery, useSettings } from "../lib/settings";
import { parseOsc7, shellDirs } from "../lib/shellDirs";
import { useShortcuts } from "../lib/shortcuts";
import { extraModes } from "../lib/terminalModes";
import { isTerminalReport } from "../lib/terminalReports";
import { fontStack, searchDecorations, sessionScheme } from "../lib/terminalSchemes";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { HIGHLIGHT_LIMIT, SearchBar } from "./SearchBar";
import { TOOLTIP_DELAY, TooltipPopup } from "./Tooltip";
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

/** Characters of a multi-line paste shown in the confirmation dialog. */
const PASTE_PREVIEW = 4000;

/**
 * How long the terminal's size shows after it changes: a second, then it fades out (see
 * `.size-overlay`). Not in a pane that opened less than `SIZE_SETTLE_MS` ago, whose terminal is
 * still settling into its place.
 */
const SIZE_SHOWN_MS = 1150;
const SIZE_SETTLE_MS = 500;

/**
 * How long the output pauses after a command is entered before the shell counts as back at its
 * prompt, maybe in another folder (which the file panel may follow).
 */
const PROMPT_SETTLE_MS = 300;

/**
 * How long a link's tooltip stays after the pointer leaves the link: xterm.js leaves and hovers
 * it again whenever its line is drawn again (the cursor blinking on it, output).
 */
const LINK_LEAVE_MS = 100;

const ignore = () => {};

/** Links open with ⌘-click on macOS and Ctrl+click elsewhere, as in other terminals. */
const opensLink = (e: MouseEvent) => (isMac ? e.metaKey : e.ctrlKey);

// Through the clipboard plugin: pasting from a menu can't raise the browser's paste event,
// and WKWebView's `navigator.clipboard.readText` asks the user with a system "Paste" button.
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
  // Fits the terminal to the pane, except while a screen from another window is drawn.
  const fitRef = useRef<{ fit(): void } | null>(null);
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
  // `link`: the link the menu was opened on.
  const [menu, setMenu] = useState<{ x: number; y: number; link?: string } | null>(null);
  // The tooltip of the link under the pointer; `uri` for links whose text may not say where they go.
  const [linkHint, setLinkHint] = useState<{ anchor: Pick<Element, "getBoundingClientRect">; uri: string | null } | null>(null);
  // Multi-line text waiting for the user to confirm the paste.
  const [pendingPaste, setPendingPaste] = useState<string | null>(null);
  // A ZMODEM transfer waiting for files or running; null when there is none.
  const [zmodemPhase, setZmodemPhase] = useState<Exclude<ZmodemPhase, "idle"> | null>(null);
  const [dragOver, setDragOver] = useState(false);
  // The size shown while the terminal is resized; `count` starts its fade-out over at each change.
  const [sizeShown, setSizeShown] = useState<{ cols: number; rows: number; count: number } | null>(null);
  // Counts the visual bells, each a new flash.
  const [flashes, setFlashes] = useState(0);
  const visibleRef = useRef(visible);
  visibleRef.current = visible;
  const wrapRef = useRef<HTMLDivElement>(null);
  const { settings, theme, update } = useSettings();
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const scheme = useMemo(
    () => sessionScheme(settings.terminal.colorScheme, theme, appearance),
    [settings.terminal.colorScheme, theme, appearance?.colorScheme, appearance?.background],
  );
  // The system asks for more contrast (macOS's Increase Contrast, Windows's contrast themes):
  // colors of the scheme too close to the background are adjusted as they are drawn.
  const moreContrast = useMediaQuery("(prefers-contrast: more), (forced-colors: active)");
  const options = {
    theme: scheme.theme,
    minimumContrastRatio: moreContrast ? 4.5 : 1,
    // A tree of the screen's lines for screen readers, and announcements of new output.
    screenReaderMode: settings.terminal.screenReader,
    fontFamily: fontStack(appearance?.fontFamily ?? settings.terminal.fontFamily, currentCjk()),
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
    // Shared by all terminals; what screen readers say for the input and for a flood of output.
    Terminal.strings.promptLabel = t("terminal.inputLabel");
    Terminal.strings.tooMuchOutput = t("terminal.tooMuchOutput");
    const term = new Terminal({
      allowProposedApi: true, // required by the unicode11 addon
      ...optionsRef.current,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
    term.loadAddon(search);
    const serialize = new SerializeAddon();
    term.loadAddon(serialize);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    // Links: addresses in the text, and those programs mark (OSC 8: `ls --hyperlink`, gcc),
    // whose text may be anything, so their tooltip shows where they go. A plain click only
    // selects or focuses, as on any other text. xterm.js only gives http and https ones.
    let link: string | null = null;
    let hintTimer: ReturnType<typeof setTimeout> | undefined;
    // The link whose tooltip is shown, and the one whose tooltip was dismissed (until left).
    let hinted: string | null = null;
    let dismissed: string | null = null;
    const activate = (e: MouseEvent, uri: string) => {
      if (opensLink(e)) void openUrl(uri).catch(console.error);
    };
    const hover = (e: MouseEvent, uri: string, marked: boolean) => {
      clearTimeout(hintTimer);
      link = uri;
      if (uri === hinted || uri === dismissed) return;
      // Below the line the pointer is on.
      const screen = term.element?.querySelector(".xterm-screen")?.getBoundingClientRect() ?? container.getBoundingClientRect();
      const height = screen.height / term.rows;
      const top = screen.top + Math.floor((e.clientY - screen.top) / height) * height;
      const rect = new DOMRect(e.clientX, top, 0, height);
      const hint = { anchor: { getBoundingClientRect: () => rect }, uri: marked ? uri : null };
      hintTimer = setTimeout(
        () => {
          hinted = uri;
          setLinkHint(hint);
        },
        hinted === null ? TOOLTIP_DELAY : 0,
      );
    };
    const leave = () => {
      clearTimeout(hintTimer);
      link = null;
      hintTimer = setTimeout(() => {
        hinted = dismissed = null;
        setLinkHint(null);
      }, LINK_LEAVE_MS);
    };
    // Clicking or scrolling dismisses it, as other tooltips are.
    const dismissHint = () => {
      if (!link) return;
      clearTimeout(hintTimer);
      dismissed = link;
      hinted = null;
      setLinkHint(null);
    };
    term.options.linkHandler = { activate, hover: (e, uri) => hover(e, uri, true), leave };
    term.loadAddon(new WebLinksAddon(activate, { hover: (e, uri) => hover(e, uri, false), leave }));
    container.addEventListener("mousedown", dismissHint, true);
    container.addEventListener("wheel", dismissHint, true);
    term.open(container);
    fit.fit();
    termRef.current = term;
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
        // With the modes programs set (mouse reporting, bracketed paste and so on), and what
        // the addon doesn't keep.
        snapshot: () => ({ data: serialize.serialize() + extraModes(term), cols: term.cols, rows: term.rows }),
        // Drawn at the size it was taken at, as the addon advises; fitted to the pane after.
        restore: (snapshot, done) => {
          term.resize(snapshot.cols, snapshot.rows);
          term.write(snapshot.data, done);
        },
        fit: () => fit.fit(),
      },
      screen: {
        size: () => ({ cols: term.cols, rows: term.rows }),
        cursor: () => {
          const buffer = term.buffer.active;
          // Past the last column while a line waits to wrap.
          return { row: buffer.cursorY + 1, column: Math.min(buffer.cursorX, term.cols - 1) + 1 };
        },
        subscribe: (listener) => {
          const events = [term.onCursorMove(listener), term.onResize(listener), term.buffer.onBufferChange(listener)];
          return () => events.forEach((e) => e.dispose());
        },
      },
      zmodem: (phase) => onZmodemRef.current(phase),
      session: () => setZmodemPhase(null),
      focus: () => term.focus(),
      hasSelection: () => term.hasSelection(),
      flash: () => setFlashes((count) => count + 1),
    });
    sessionRef.current = session;
    fitRef.current = {
      fit: () => {
        if (!session.isArriving) fit.fit();
      },
    };

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

    // Ctrl+Shift+C / Ctrl+V and friends outside macOS (see `clipboardKeyOf`).
    term.attachCustomKeyEventHandler((e) => {
      // The menu key and Shift+F10 open the context menu, at the cursor (where xterm.js keeps
      // its text field), whatever the program does with the mouse and right-clicks do. The
      // browser may fire `contextmenu` for them as well, which can't tell them from a click.
      if (isMenuKey(e)) {
        if (e.type === "keydown") {
          e.preventDefault();
          openedByMenuKey();
          const at = term.textarea?.getBoundingClientRect() ?? container.getBoundingClientRect();
          setMenu({ x: at.left, y: at.bottom });
        }
        return false;
      }
      const key = clipboardKeyOf(e);
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
      if (followsMenuKey(e)) return;
      e.preventDefault();
      // Programs using the mouse get the click; Shift+right-click still opens the menu.
      if (term.modes.mouseTrackingMode !== "none" && !e.shiftKey) return;
      if (settingsRef.current.terminal.rightClick === "paste" && !e.shiftKey) {
        readText().then(pasteRef.current, ignore);
        return;
      }
      setMenu({ x: e.clientX, y: e.clientY, link: link ?? undefined });
    };
    container.addEventListener("contextmenu", onContextMenu);

    const opened = Date.now();
    let hideSize: ReturnType<typeof setTimeout> | undefined;
    const showSize = (cols: number, rows: number) => {
      // Out of sight (a hidden tab resized with the window), nobody would see it.
      if (!visibleRef.current || Date.now() - opened < SIZE_SETTLE_MS) return;
      setSizeShown((shown) => ({ cols, rows, count: (shown?.count ?? 0) + 1 }));
      clearTimeout(hideSize);
      hideSize = setTimeout(() => setSizeShown(null), SIZE_SHOWN_MS);
    };

    // Once a command is entered (Enter, or a paste with a line break), the next pause in the
    // output tells the file panel that the shell may have changed folders.
    let entered = false;
    let promptSettle: ReturnType<typeof setTimeout> | undefined;
    const waitForPrompt = () => {
      clearTimeout(promptSettle);
      promptSettle = setTimeout(() => {
        entered = false;
        shellDirs.mayHaveMoved(paneKey);
      }, PROMPT_SETTLE_MS);
    };

    const subscriptions: IDisposable[] = [
      // Reports are for the program in this pane only, so not synced to other panes.
      term.onData((data) => {
        const report = isTerminalReport(data);
        if (!report && data.includes("\r")) {
          entered = true;
          waitForPrompt();
        }
        if (session.input(data) && !pasting && !report) onInputRef.current(data);
      }),
      term.onWriteParsed(() => {
        if (entered) waitForPrompt();
      }),
      // The folder the shell reports, where it is set up to (OSC 7).
      term.parser.registerOscHandler(7, (data) => {
        const path = parseOsc7(data);
        if (path) shellDirs.report(paneKey, path);
        return true;
      }),
      term.onBinary((data) => session.inputBinary(data)),
      term.onResize(({ cols, rows }) => {
        session.resize(cols, rows);
        showSize(cols, rows);
      }),
      term.onTitleChange((title) => onTitleRef.current(title)),
      term.onBell(() => session.bell()),
    ];
    const observer = new ResizeObserver(() => fitRef.current?.fit());
    observer.observe(container);
    // A tick later: React's StrictMode (in development) mounts, unmounts and mounts again at
    // once, which would open a backend session (and a log file, or a serial device that is
    // then busy) for the first mount too.
    const start = setTimeout(() => session.start(), 0);

    return () => {
      clearTimeout(start);
      clearTimeout(hideSize);
      clearTimeout(promptSettle);
      clearTimeout(hintTimer);
      container.removeEventListener("mousedown", dismissHint, true);
      container.removeEventListener("wheel", dismissHint, true);
      shellDirs.forget(paneKey);
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
  const { macOptionIsMeta, rightClickSelectsWord, minimumContrastRatio, screenReaderMode } = options;
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
    term.options.minimumContrastRatio = minimumContrastRatio;
    term.options.screenReaderMode = screenReaderMode;
    fitRef.current?.fit();
  }, [
    termTheme,
    fontFamily,
    fontSize,
    cursorStyle,
    cursorBlink,
    scrollback,
    macOptionIsMeta,
    rightClickSelectsWord,
    minimumContrastRatio,
    screenReaderMode,
  ]);

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
    let stopWaiting = () => {};
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      // Behind a dialog (the pane before it closed while the settings were open), the
      // terminal would take Escape and Tab from the dialog: it waits for the dialog to
      // close, and then takes the focus unless something else has it.
      if (!isDialogOpen()) termRef.current?.focus();
      else
        stopWaiting = afterDialogs(() => {
          const lost = !document.activeElement || document.activeElement === document.body;
          if (lost) termRef.current?.focus();
        });
    });
    return () => {
      cancelAnimationFrame(frame);
      stopWaiting();
    };
  }, [active]);

  // A dialog (settings) above the terminal has the shortcut to itself. Copying and pasting
  // from the app menu: their keys are the terminal's own (see `clipboardKeyOf`).
  useShortcuts(
    (e) => {
      const term = termRef.current;
      if (matches(e, "find")) setSearchKey((key) => key + 1);
      else if (chosenAction(e) === "copy") {
        if (term?.hasSelection()) copyText(term.getSelection());
      } else if (chosenAction(e) === "paste") void readText().then(pasteRef.current, ignore);
      else return false;
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

  // A file dialog that `rz` or `sz` opened while the app was locked, waiting to be shown:
  // shown at once, it would be over the lock screen.
  const pendingDialogRef = useRef<() => void>(null);
  onZmodemRef.current = (phase) => {
    const id = sessionRef.current?.id;
    if (id == null) return;
    setZmodemPhase(phase === "idle" ? null : phase);
    pendingDialogRef.current?.();
    pendingDialogRef.current = null;
    if (phase === "chooseFiles") pendingDialogRef.current = whenUnlocked(chooseFiles);
    else if (phase === "chooseDestination") {
      if (settings.zmodem.receive === "downloads") saveReceived(null);
      else if (settings.zmodem.receive === "chooseFolder") pendingDialogRef.current = whenUnlocked(chooseFolder);
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
      // Native events, which the lock screen doesn't stop: nothing is dropped while locked.
      if (isAppLocked()) {
        setDragOver(false);
        return;
      }
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

  const menuItems = (link?: string): MenuItem[] => {
    const term = termRef.current!;
    const linkItems: MenuItem[] = link
      ? [
          { label: t("terminal.menu.openLink"), onSelect: () => void openUrl(link).catch(console.error) },
          { label: t("terminal.menu.copyLink"), onSelect: () => copyText(link) },
          "separator",
        ]
      : [];
    return [
      ...linkItems,
      {
        label: t("terminal.menu.copy"),
        shortcut: shortcutLabel("copy"),
        disabled: !term.hasSelection(),
        onSelect: () => copyText(term.getSelection()),
      },
      {
        label: t("terminal.menu.paste"),
        shortcut: shortcutLabel("paste"),
        onSelect: () => void readText().then(pasteRef.current, ignore),
      },
      { label: t("terminal.menu.selectAll"), shortcut: shortcutLabel("selectAll"), onSelect: () => term.selectAll() },
      "separator" as const,
      { label: t("terminal.menu.find"), shortcut: shortcutLabel("find"), onSelect: () => setSearchKey((key) => key + 1) },
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
      {flashes > 0 && <div key={flashes} className="bell-flash" aria-hidden="true" style={{ background: scheme.theme.foreground }} />}
      {sizeShown && (
        // The status bar has the size for screen readers.
        <div key={sizeShown.count} className="size-overlay" aria-hidden="true" style={{ fontFamily }}>
          {t("terminal.size", { cols: sizeShown.cols, rows: sizeShown.rows })}
        </div>
      )}
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
          items={menuItems(menu.link)}
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
      {linkHint && (
        <TooltipPopup
          anchor={linkHint.anchor}
          text={[linkHint.uri, t(isMac ? "terminal.link.openHintMac" : "terminal.link.openHint")].filter(Boolean).join("\n")}
        />
      )}
      {searchKey > 0 && searchRef.current && (
        <SearchBar addon={searchRef.current} decorations={searchDecorations(scheme)} focusKey={searchKey} onClose={closeSearch} />
      )}
    </div>
  );
}
