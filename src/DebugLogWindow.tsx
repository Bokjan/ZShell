import { useEffect, useEffectEvent, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Terminal } from "@xterm/xterm";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import "@xterm/xterm/css/xterm.css";

import { ContextMenu, type MenuItem } from "./components/ContextMenu";
import { IconButton } from "./components/IconButton";
import { TrashIcon } from "./components/icons";
import { RadioGroup } from "./components/RadioGroup";
import { Select } from "./components/Select";
import { NewOutputButton } from "./components/terminal/NewOutputButton";
import { followReadBack } from "./components/terminal/readBackView";
import { WindowControls } from "./components/WindowControls";
import { useLatest } from "./hooks/latest";
import { useLocale } from "./hooks/locale";
import { useMediaQuery, useSettings } from "./hooks/settings";
import { DRAG_REGION, useTitleBar } from "./hooks/titleBar";
import { useWindowEvent } from "./hooks/windowEvents";
import { DEBUG_LOG_MENU_ACTIONS } from "./ipc/bindings";
import { debugLog } from "./ipc/debugLog";
import { listenHere } from "./ipc/events";
import { windows } from "./ipc/windows";
import { announce } from "./lib/announce";
import {
  addEntries,
  clearUpTo,
  LEVELS,
  nextWriting,
  NO_LOGS,
  plainLine,
  shown,
  terminalLine,
  type DebugLogFilter,
  type DebugLogs,
  type DebugLogSource,
  type Written,
} from "./lib/debugLog";
import { fontLigatureRules } from "./lib/fontLigatureRules";
import { isKey } from "./lib/keys";
import { matches, type MenuAction } from "./lib/keymap";
import { ligatureRanges } from "./lib/ligatures";
import type { ReadBack } from "./lib/readBack";
import { isMac } from "./lib/platform";
import { FullWidthFitAddon } from "./lib/terminalFit";
import { fontStack, sessionScheme } from "./lib/terminalSchemes";
import { drawWithWebgl } from "./lib/webgl";

/** Rows the terminal keeps: every record the backend keeps, and room for those that wrap. */
const SCROLLBACK = 50_000;
/** How long the filter waits for the next key before it shows what matches. */
const FILTER_DELAY = 150;
/** Hides the cursor, which a reset shows again: the terminal takes no input. */
const HIDE_CURSOR = "\x1b[?25l";

const NO_FILTER: DebugLogFilter = { level: "debug", query: "" };

/**
 * The menu bar's items sent here (macOS, see `menu::chosen`): those this page handles, and the
 * debug log's own, which finds it open. The others act on an app window.
 */
const MENU_ACTIONS: readonly MenuAction[] = DEBUG_LOG_MENU_ACTIONS;

/**
 * The debug log window (`debug-log`, opened from the Help menu): the backend's and the pages'
 * records (see `debug_log/` in the backend), one side at a time, filtered by level and text.
 * They are written to a read-only terminal, in the terminal's font and colors, which draws
 * only what is in view, selects across what scrolled away and wraps long lines again as the
 * window is resized; new records come at the end, and the view follows them unless scrolled
 * back. Each side keeps its own filter.
 */
export function DebugLogWindow() {
  useTitleBar();
  const { t } = useTranslation();
  const { settings, theme, screenReader } = useSettings();
  const { cjk, shortcutLabel } = useLocale();
  const [logs, setLogs] = useState<DebugLogs>(NO_LOGS);
  const [source, setSource] = useState<DebugLogSource>("backend");
  const [filters, setFilters] = useState<Record<DebugLogSource, DebugLogFilter>>({ backend: NO_FILTER, frontend: NO_FILTER });
  // What is typed into the filter field, which the filter takes once typing pauses.
  const [typed, setTyped] = useState<Record<DebugLogSource, string>>({ backend: "", frontend: "" });
  const [terminal, setTerminal] = useState<Terminal | null>(null);
  // Follows the view reading back through the records, for the button back to the end.
  const [readBack, setReadBack] = useState<ReadBack | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  // How many of the side's records the filter shows, counted as they are written.
  const [shownCount, setShownCount] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FullWidthFitAddon | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const writtenRef = useRef<Written | null>(null);
  const filter = filters[source];
  const query = typed[source];

  const scheme = sessionScheme(settings.terminal.colorScheme, theme, undefined);
  const moreContrast = useMediaQuery("(prefers-contrast: more), (forced-colors: active)");
  const options = {
    theme: scheme.theme,
    minimumContrastRatio: moreContrast ? 4.5 : 1,
    screenReaderMode: screenReader,
    fontFamily: fontStack(settings.terminal.fontFamily, cjk),
    fontSize: settings.terminal.fontSize,
  };
  const optionsRef = useLatest(options);

  useEffect(() => {
    void windows.setTitle(t("debugLog.title")).catch(console.error);
  }, [t]);

  // The records so far, once the new ones are heard: those that come meanwhile are in both.
  useEffect(() => {
    let current = true;
    const unlisten = listenHere("debug-log-added", (e) => setLogs((logs) => addEntries(logs, e.payload)));
    void unlisten
      .then(() => debugLog.snapshot())
      .then((entries) => current && setLogs((logs) => addEntries(logs, entries)))
      .catch(console.error);
    return () => {
      current = false;
      void unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const container = containerRef.current!;
    const term = new Terminal({
      allowProposedApi: true, // required by the unicode11 addon
      disableStdin: true,
      cursorBlink: false,
      cursorInactiveStyle: "none",
      scrollback: SCROLLBACK,
      ...optionsRef.current,
    });
    const fit = new FullWidthFitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    // The terminal's copy keys copy the selection; ⌘C is the menu's, which copies it too.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !(matches(e, "copy") || matches(e, "copySelection")) || !term.hasSelection()) return true;
      void writeText(term.getSelection()).catch(console.error);
      return false;
    });
    term.open(container);
    fit.fit();
    term.write(HIDE_CURSOR);
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => fit.fit());
    });
    observer.observe(container);
    const following = followReadBack(term, container, () => {});
    setReadBack(following.readBack);
    termRef.current = term;
    fitRef.current = fit;
    setTerminal(term);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      following.dispose();
      setReadBack(null);
      termRef.current = null;
      fitRef.current = null;
      writtenRef.current = null;
      setTerminal(null);
      term.dispose();
    };
  }, [optionsRef]);

  // Appearance and font changes, on the running terminal.
  const { theme: termTheme, fontFamily, fontSize, minimumContrastRatio, screenReaderMode } = options;
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = termTheme;
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    term.options.minimumContrastRatio = minimumContrastRatio;
    term.options.screenReaderMode = screenReaderMode;
    fitRef.current?.fit();
  }, [terminal, termTheme, fontFamily, fontSize, minimumContrastRatio, screenReaderMode]);

  useEffect(() => (terminal ? drawWithWebgl(terminal) : undefined), [terminal]);

  // The font's ligatures as the terminal draws them (see `TerminalView`).
  const ligatures = settings.terminal.ligatures;
  useEffect(() => {
    if (!terminal || ligatures === "off") return;
    const redraw = () => terminal.refresh(0, terminal.rows - 1);
    const rules = ligatures === "full" ? fontLigatureRules(fontFamily, redraw) : null;
    const joiner = terminal.registerCharacterJoiner(rules ? rules.ranges : ligatureRanges);
    redraw();
    return () => {
      rules?.release();
      terminal.deregisterCharacterJoiner(joiner);
      redraw();
    };
  }, [terminal, ligatures, fontFamily]);

  // The filter takes what is typed once typing pauses.
  useEffect(() => {
    if (query === filter.query) return;
    const timer = setTimeout(() => setFilters((filters) => ({ ...filters, [source]: { ...filters[source], query } })), FILTER_DELAY);
    return () => clearTimeout(timer);
  }, [query, filter.query, source]);

  // New records are written at the end; anything else (another side or filter, records that
  // came out of order, a cleared side) writes the terminal anew (see `nextWriting`).
  const list = logs[source];
  useEffect(() => {
    if (!terminal) return;
    const writing = nextWriting(writtenRef.current, logs, source, filter);
    if (writing.anew) {
      terminal.reset();
      terminal.write(HIDE_CURSOR);
    }
    const text = writing.entries.map(terminalLine).join("");
    if (text) terminal.write(text);
    writtenRef.current = writing.written;
    setShownCount(writing.written.shown);
  }, [terminal, logs, source, filter]);

  const focusFilter = () => {
    searchRef.current?.focus();
    searchRef.current?.select();
  };
  const close = () => void getCurrentWindow().close().catch(console.error);

  // The keys of finding and closing, as in the other windows, and the usual ⌘F / Ctrl+F (no
  // shell here to take it). On macOS the menu has them first (`menu-action`).
  const onKeyDown = useEffectEvent((e: KeyboardEvent) => {
    const findKey = (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey) && !e.altKey && !e.shiftKey && isKey(e, "f");
    if (findKey || matches(e, "find")) {
      e.preventDefault();
      focusFilter();
    } else if (matches(e, "closeTab") || matches(e, "closeWindow")) {
      e.preventDefault();
      close();
    }
  });
  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKeyDown(e);
    window.addEventListener("keydown", listener, true);
    return () => window.removeEventListener("keydown", listener, true);
  }, []);
  useWindowEvent("menu-action", (action) => {
    if (!MENU_ACTIONS.includes(action as MenuAction)) return;
    if (action === "find") focusFilter();
    else if (action === "closeTab" || action === "closeWindow") close();
  });

  const copyText = (text: string) => void writeText(text).catch(console.error);
  // Filtered here rather than as the window renders, which new records do ten times a second.
  const copyShown = () => {
    const visible = list.filter((entry) => shown(entry, filter));
    const text = visible.map(plainLine).join("\n");
    writeText(text).then(() => announce(t("debugLog.copied", { count: visible.length })), console.error);
  };
  const scrollToEnd = () => {
    termRef.current?.scrollToBottom();
    termRef.current?.focus();
  };
  const menuItems = (term: Terminal): MenuItem[] => [
    {
      label: t("terminal.menu.copy"),
      shortcut: shortcutLabel("copy"),
      disabled: !term.hasSelection(),
      onSelect: () => copyText(term.getSelection()),
    },
    { label: t("terminal.menu.selectAll"), shortcut: shortcutLabel("selectAll"), onSelect: () => term.selectAll() },
    "separator",
    { label: t("debugLog.copyShown"), disabled: shownCount === 0, onSelect: copyShown },
    { label: t("debugLog.clear"), disabled: list.length === 0, onSelect: () => clear() },
  ];
  const clear = () => {
    debugLog.clear(source).then((upTo) => {
      setLogs((logs) => clearUpTo(logs, source, upTo));
      announce(t("debugLog.cleared"));
    }, console.error);
  };

  return (
    <div className="debug-log">
      <div className="debug-log-bar" {...DRAG_REGION}>
        <RadioGroup<DebugLogSource>
          className="segmented debug-log-sources"
          label={t("debugLog.source")}
          value={source}
          options={[
            { value: "backend", content: t("debugLog.backend") },
            { value: "frontend", content: t("debugLog.frontend") },
          ]}
          onChange={setSource}
        />
        <Select
          aria-label={t("debugLog.level")}
          value={filter.level}
          widest
          options={LEVELS.map((level) => ({ value: level, label: t(`debugLog.levels.${level}`) }))}
          onChange={(level) => setFilters((filters) => ({ ...filters, [source]: { ...filters[source], level } }))}
        />
        <input
          ref={searchRef}
          className="debug-log-filter"
          type="text"
          value={query}
          placeholder={t("debugLog.filter")}
          aria-label={t("debugLog.filter")}
          autoComplete="off"
          onChange={(e) => setTyped((typed) => ({ ...typed, [source]: e.target.value }))}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) {
              e.preventDefault();
              setTyped((typed) => ({ ...typed, [source]: "" }));
            }
          }}
        />
        <span className="debug-log-count" {...DRAG_REGION}>
          {t("debugLog.count", { shown: shownCount, total: list.length })}
        </span>
        <IconButton className="icon-button" label={t("debugLog.clear")} onClick={clear} disabled={list.length === 0}>
          <TrashIcon />
        </IconButton>
        {!isMac && <WindowControls />}
      </div>
      <div className="debug-log-view" style={{ background: scheme.theme.background }}>
        {/* Ligatures off: the DOM renderer would join runs of one style (see `.terminal-view.no-ligatures`). */}
        <div
          className="debug-log-terminal"
          ref={containerRef}
          style={ligatures === "off" ? { fontVariantLigatures: "none" } : undefined}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY });
          }}
        />
        {readBack && <NewOutputButton readBack={readBack} visible onClick={scrollToEnd} />}
      </div>
      {menu && terminal && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(terminal)}
          onClose={() => {
            setMenu(null);
            terminal.focus();
          }}
        />
      )}
    </div>
  );
}
