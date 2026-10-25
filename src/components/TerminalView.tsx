import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Terminal } from "@xterm/xterm";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import "@xterm/xterm/css/xterm.css";

import { conptyBuild, type ProfileAppearance } from "../ipc/api";
import { afterDialogs, isDialogOpen } from "../hooks/dialogs";
import { chosenAction, matches } from "../lib/keymap";
import type { PaneSession } from "../lib/paneSession";
import { isMac } from "../lib/platform";
import type { PasteTarget, SessionRegistry } from "../lib/sessionRegistry";
import { useMediaQuery, useSettings } from "../hooks/settings";
import { shellDirs } from "../lib/shellDirs";
import { announce } from "../lib/announce";
import { Disposables } from "../lib/disposables";
import { PromptWatch } from "../lib/promptWatch";
import { handleOsc } from "../lib/terminalOsc";
import { useShortcuts } from "../hooks/shortcuts";
import { fontLigatureRules } from "../lib/fontLigatureRules";
import { ligatureRanges } from "../lib/ligatures";
import { FullWidthFitAddon } from "../lib/terminalFit";
import { isTerminalReport } from "../lib/terminalReports";
import type { ReadBack } from "../lib/readBack";
import { fontStack, searchDecorations, sessionScheme } from "../lib/terminalSchemes";
import { drawWithWebgl } from "../lib/webgl";
import { createStore } from "../lib/store";
import { useLatest } from "../hooks/latest";
import { useLocale } from "../hooks/locale";
import { ConfirmDialog } from "./ConfirmDialog";
import { ContextMenu, type MenuItem } from "./ContextMenu";
import { HIGHLIGHT_LIMIT, SearchBar } from "./SearchBar";
import { TooltipPopup } from "./Tooltip";
import { attachClipboard, copyText, pasteClipboard, type MenuAt } from "./terminal/clipboard";
import { attachLinks, type LinkHint } from "./terminal/links";
import { NewOutputButton } from "./terminal/NewOutputButton";
import { terminalPort, terminalScreen } from "./terminal/port";
import { followReadBack } from "./terminal/readBackView";
import { followSize, showSize } from "./terminal/size";
import { SizeOverlay, type SizeShown } from "./terminal/SizeOverlay";
import { useZmodem } from "./terminal/useZmodem";

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
  /** The other panes a paste goes to as well (syncing): each pastes it its own way. */
  pasteTargets(): PasteTarget[];
  /** Added to the end of the context menu when it opens (pane actions, quick commands). */
  menuItems(): MenuItem[];
  /** A local shell, which on Windows runs in ConPTY. */
  local: boolean;
  /** The session's own colors and font, over the settings. */
  appearance?: ProfileAppearance;
}

/** Characters of a multi-line paste shown in the confirmation dialog. */
const PASTE_PREVIEW = 4000;

const ignore = () => {};

const lineCount = (text: string) => text.replace(/(\r\n|\r|\n)$/, "").split(/\r\n|\r|\n/).length;

export function TerminalView({
  paneKey,
  sessions,
  visible,
  active,
  onTitle,
  onInput,
  pasteTargets,
  menuItems: extraMenuItems,
  local,
  appearance,
}: Props) {
  const { t } = useTranslation();
  const { shortcutLabel, cjk } = useLocale();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  // Fits the terminal to the pane, except while a screen from another window is drawn.
  const fitRef = useRef<{ fit(): void } | null>(null);
  // Resized while hidden: to fit once shown.
  const unfittedRef = useRef(false);
  const searchRef = useRef<SearchAddon | null>(null);
  // The same two for rendering (the context menu, the search bar), which can't read refs.
  const [terminal, setTerminal] = useState<Terminal | null>(null);
  const [searchAddon, setSearchAddon] = useState<SearchAddon | null>(null);
  const sessionsRef = useLatest(sessions);
  const onTitleRef = useLatest(onTitle);
  const onInputRef = useLatest(onInput);
  const pasteTargetsRef = useLatest(pasteTargets);
  /** Pastes text, asking first if it would run several commands; set while the terminal exists. */
  const pasteRef = useRef<(text: string) => void>(ignore);
  /** Pastes into this pane and the panes synced with it, without asking. */
  const pasteAllRef = useRef<(text: string) => void>(ignore);
  /** The pane's session, for answering ZMODEM transfers. */
  const sessionRef = useRef<PaneSession | null>(null);
  // Incremented by the find shortcut; 0 means the search bar is closed.
  const [searchKey, setSearchKey] = useState(0);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const [linkHint, setLinkHint] = useState<LinkHint | null>(null);
  // Multi-line text waiting for the user to confirm the paste.
  const [pendingPaste, setPendingPaste] = useState<string | null>(null);
  // The size shown while the terminal is resized; `count` starts its fade-out over at each change.
  const [sizeShown] = useState(() => createStore<SizeShown>({ size: null }));
  // Counts the visual bells, each a new flash.
  const [flashes, setFlashes] = useState(0);
  // Follows the view reading back through the scrollback, for the button back to the end.
  const [readBack, setReadBack] = useState<ReadBack | null>(null);
  // Counts the notices that output removes the lines being read back; 0 while none shows.
  const [trimmingShown, setTrimmingShown] = useState(0);
  const visibleRef = useLatest(visible);
  const wrapRef = useRef<HTMLDivElement>(null);
  const transfer = useZmodem({ sessionRef, active, wrapRef, focus: () => termRef.current?.focus() });
  const { settings, theme, screenReader, update } = useSettings();
  const settingsRef = useLatest(settings);
  const tRef = useLatest(t);
  /** Puts back the terminal's own pointer shape, over any a program set (OSC 22). */
  const resetPointerRef = useRef<() => void>(ignore);
  // Only these two of the session's appearance choose its colors.
  const ownScheme = appearance?.colorScheme;
  const ownBackground = appearance?.background;
  const scheme = useMemo(
    () => sessionScheme(settings.terminal.colorScheme, theme, { colorScheme: ownScheme, background: ownBackground }),
    [settings.terminal.colorScheme, theme, ownScheme, ownBackground],
  );
  // The system asks for more contrast (macOS's Increase Contrast, Windows's contrast themes):
  // colors of the scheme too close to the background are adjusted as they are drawn.
  const moreContrast = useMediaQuery("(prefers-contrast: more), (forced-colors: active)");
  const options = {
    theme: scheme.theme,
    minimumContrastRatio: moreContrast ? 4.5 : 1,
    // A tree of the screen's lines for screen readers, and announcements of new output.
    screenReaderMode: screenReader,
    fontFamily: fontStack(appearance?.fontFamily ?? settings.terminal.fontFamily, cjk),
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
  const optionsRef = useLatest(options);

  // Shared by all terminals: what screen readers say for the input and for a flood of output,
  // in the interface's language, which may change while the terminal is open. Before the
  // terminal is made, which names its input then.
  useEffect(() => {
    Terminal.strings.promptLabel = t("terminal.inputLabel");
    Terminal.strings.tooMuchOutput = t("terminal.tooMuchOutput");
    termRef.current?.textarea?.setAttribute("aria-label", Terminal.strings.promptLabel);
  }, [t]);

  useEffect(() => {
    const container = containerRef.current!;
    const term = new Terminal({
      allowProposedApi: true, // required by the unicode11 addon
      ...optionsRef.current,
    });
    const fit = new FullWidthFitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
    term.loadAddon(search);
    const serialize = new SerializeAddon();
    term.loadAddon(serialize);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    // Let go of in the reverse order, before the session and the terminal.
    const held = new Disposables();
    const links = held.add(attachLinks(term, container, setLinkHint));
    term.open(container);
    fit.fit();
    termRef.current = term;
    searchRef.current = search;
    setTerminal(term);
    setSearchAddon(search);

    const session = sessionsRef.current.attach(paneKey, {
      port: terminalPort(term, serialize, fit),
      screen: terminalScreen(term),
      zmodem: (phase) => transfer.onPhaseRef.current(phase),
      paste: {
        bracketed: () => term.modes.bracketedPasteMode,
        paste: (text) => {
          if (text && !session.isClosed) term.paste(text);
        },
      },
      session: () => {
        transfer.reset();
        resetPointerRef.current();
      },
      focus: () => term.focus(),
      hasSelection: () => term.hasSelection(),
      flash: () => setFlashes((count) => count + 1),
    });
    sessionRef.current = session;

    const clipboard = held.add(
      attachClipboard(term, container, {
        closed: () => session.isClosed,
        pasteTargets: () => pasteTargetsRef.current(),
        settings: () => settingsRef.current.terminal,
        confirm: setPendingPaste,
        openMenu: setMenu,
        link: links.current,
      }),
    );
    pasteRef.current = clipboard.paste;
    pasteAllRef.current = clipboard.pasteAll;

    const prompt = held.add(new PromptWatch(() => shellDirs.mayHaveMoved(paneKey)));
    held.defer(() => shellDirs.forget(paneKey));
    const osc = held.add(
      handleOsc(term, {
        folder: (path) => shellDirs.report(paneKey, path),
        copyLimit: () => {
          const { clipboardWrite, clipboardLimit } = settingsRef.current.terminal;
          return clipboardWrite ? clipboardLimit * 1024 : null;
        },
        copy: (text) => void writeText(text).then(() => announce(tRef.current("announce.copied")), console.error),
        progress: (report) => session.progress(report),
        notify: (notice) => session.notify(notice),
        // A link's hand and column selection's crosshair still show over it (see `.program-pointer`).
        pointer: (shape) => {
          container.classList.toggle("program-pointer", shape !== "");
          container.style.setProperty("--program-pointer", shape || null);
        },
      }),
    );
    resetPointerRef.current = osc.resetPointer;

    const { readBack } = held.add(
      followReadBack(term, container, (shown) => {
        if (!shown) setTrimmingShown(0);
        else {
          if (visibleRef.current) announce(tRef.current("terminal.trimmingView"));
          setTrimmingShown((count) => count + 1);
        }
      }),
    );
    setReadBack(readBack);
    held.defer(() => setReadBack(null));

    held.add(term.onResize(({ cols, rows }) => session.resize(cols, rows)));
    held.add(showSize(term, sizeShown, () => visibleRef.current));
    // Reports are for the program in this pane only, so not synced to other panes.
    held.add(
      term.onData((data) => {
        const report = isTerminalReport(data);
        if (!report) prompt.typed(data);
        if (session.input(data) && !clipboard.pasting() && !report) onInputRef.current(data);
      }),
    );
    held.add(term.onWriteParsed(() => prompt.output()));
    held.add(term.onBinary((data) => session.inputBinary(data)));
    held.add(term.onTitleChange((title) => onTitleRef.current(title)));
    held.add(term.onBell(() => session.bell()));

    fitRef.current = held.add(
      followSize(term, fit, container, {
        arriving: () => session.isArriving,
        visible: () => visibleRef.current,
        unfitted: () => (unfittedRef.current = true),
      }),
    );
    // A tick later: React's StrictMode (in development) mounts, unmounts and mounts again at
    // once, which would open a backend session (and a log file, or a serial device that is
    // then busy) for the first mount too.
    held.timer().start(() => session.start(), 0);

    return () => {
      pasteRef.current = pasteAllRef.current = resetPointerRef.current = ignore;
      held.dispose();
      sessionsRef.current.detach(paneKey, session);
      sessionRef.current = null;
      term.dispose();
      termRef.current = fitRef.current = searchRef.current = null;
      setTerminal(null);
      setSearchAddon(null);
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

  // The sequences drawn together, as ligatures in fonts that have them (see `lib/ligatures`).
  // The DOM renderer joins every run of cells in one style, so it shows all the font's
  // ligatures while they are on, and none while off (`.no-ligatures`).
  const ligatures = settings.terminal.ligatures;
  useEffect(() => {
    if (!terminal || ligatures === "off") return;
    const redraw = () => terminal.refresh(0, terminal.rows - 1);
    // The font's own rules, once read (`lib/fontLigatureRules`).
    const rules = ligatures === "full" ? fontLigatureRules(fontFamily, redraw) : null;
    const joiner = terminal.registerCharacterJoiner(rules ? rules.ranges : ligatureRanges);
    redraw();
    return () => {
      rules?.release();
      // Not once the terminal is gone (unmounting), whose cleanup runs before this one.
      if (termRef.current !== terminal) return;
      terminal.deregisterCharacterJoiner(joiner);
      redraw();
    };
  }, [terminal, ligatures, fontFamily]);

  // ConPTY redraws the screen as the terminal is resized, and before Windows build 21376 also
  // rewraps its lines: xterm.js does neither itself then (see `windowsPty`).
  useEffect(() => {
    if (!terminal || !local) return;
    let current = true;
    void conptyBuild().then((buildNumber) => {
      if (current && buildNumber !== null) terminal.options.windowsPty = { backend: "conpty", buildNumber };
    });
    return () => {
      current = false;
    };
  }, [terminal, local]);

  // Before the page is painted, so that it doesn't show the terminal at its old size first.
  useLayoutEffect(() => {
    if (!visible || !unfittedRef.current) return;
    unfittedRef.current = false;
    fitRef.current?.fit();
  }, [visible]);

  // The web view allows only so many WebGL contexts (about 16), and drops the oldest beyond
  // that: terminals of hidden tabs release theirs (drawing with the DOM renderer meanwhile)
  // and create one again when shown. A lost context is replaced the next time too. Past a
  // budget, the visible terminals used least recently give theirs up too, until focused or
  // there is room again (`lib/webglBudget`).
  useEffect(() => {
    const term = termRef.current;
    if (!term || !visible) return;
    return drawWithWebgl(term);
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
      } else if (chosenAction(e) === "paste") pasteClipboard(pasteRef.current);
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

  const scrollToEnd = () => {
    termRef.current?.scrollToBottom();
    focus();
  };

  const menuItems = (term: Terminal, link?: string): MenuItem[] => {
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
        onSelect: () => pasteClipboard(pasteRef.current),
      },
      { label: t("terminal.menu.selectAll"), shortcut: shortcutLabel("selectAll"), onSelect: () => term.selectAll() },
      "separator" as const,
      { label: t("terminal.menu.find"), shortcut: shortcutLabel("find"), onSelect: () => setSearchKey((key) => key + 1) },
      "separator" as const,
      { label: t("terminal.menu.clear"), onSelect: () => term.clear() },
      {
        label: t("terminal.menu.reset"),
        onSelect: () => {
          term.reset();
          resetPointerRef.current();
        },
      },
      ...extraMenuItems(),
    ];
  };

  const confirmPaste = (text: string, dontAskAgain: boolean) => {
    setPendingPaste(null);
    if (dontAskAgain) update((settings) => ({ ...settings, terminal: { ...settings.terminal, confirmMultilinePaste: false } }));
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
      <div className={ligatures === "off" ? "terminal-view no-ligatures" : "terminal-view"} ref={containerRef} />
      {flashes > 0 && <div key={flashes} className="bell-flash" aria-hidden="true" style={{ background: scheme.theme.foreground }} />}
      <SizeOverlay shown={sizeShown} fontFamily={fontFamily} />
      {readBack && <NewOutputButton readBack={readBack} visible={visible} onClick={scrollToEnd} />}
      {trimmingShown > 0 && (
        // Announced as well, when it shows.
        <div key={trimmingShown} className="trimming-hint" aria-hidden="true">
          {t("terminal.trimmingView")}
        </div>
      )}
      {transfer.bar}
      {menu && terminal && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(terminal, menu.link)}
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
      {searchKey > 0 && searchAddon && (
        <SearchBar
          addon={searchAddon}
          decorations={searchDecorations(scheme)}
          focusKey={searchKey}
          visible={visible}
          onClose={closeSearch}
        />
      )}
    </div>
  );
}
