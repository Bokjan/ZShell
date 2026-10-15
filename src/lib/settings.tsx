import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { followLanguageSetting } from "../i18n";
import type { Appearance, Settings, TextSize } from "./bindings";
import { latestSave } from "./latestSave";
import { isWindows } from "./platform";
import { setSystemFont } from "./terminalSchemes";
import { useConfigChanged, useWindowEvent } from "./windows";

export type {
  Appearance,
  CursorStyle,
  FileSettings,
  LogFormat,
  LogSettings,
  RightClick,
  ScreenReaderMode,
  Settings,
  SidebarSettings,
  StatusBarSettings,
  TabSettings,
  TerminalSettings,
  TextSize,
  ZmodemReceive,
  ZmodemSettings,
} from "./bindings";

const TEXT_SCALES: Record<TextSize, number> = { normal: 1, large: 1.15, larger: 1.3 };

/** The terminal font sizes allowed, in the settings and in sessions. */
export const FONT_SIZE_MIN = 6;
export const FONT_SIZE_MAX = 48;

export const DEFAULT_LOG_FILE_NAME = "{session}_{date}_{time}.log";

/** How long a setting typed into a field waits for the next key before it is saved. */
export const TYPING_SAVE_DELAY = 500;

export const DEFAULT_SETTINGS: Settings = {
  language: "system",
  appearance: "system",
  textSize: "normal",
  appFont: "",
  statusBar: { show: true },
  terminal: {
    colorScheme: "auto",
    fontFamily: "",
    fontSize: 13,
    cursorStyle: "block",
    cursorBlink: true,
    scrollback: 5000,
    copyOnSelect: false,
    rightClick: "menu",
    confirmMultilinePaste: true,
    optionAsMeta: false,
    screenReaderMode: "auto",
    bellAlert: true,
    bellSound: true,
    bellFlash: false,
  },
  tabs: { followRemoteTitle: true, confirmClose: true, markOutput: true },
  sidebar: { showRecent: true },
  files: { downloadDirectory: "", editor: "" },
  zmodem: { receive: "ask" },
  logs: { directory: "", fileName: DEFAULT_LOG_FILE_NAME, format: "text", timestamps: false, autoLocal: false, keepDays: 0 },
  updates: { check: true, skipped: null },
};

interface SettingsContextValue {
  settings: Settings;
  /** The appearance in effect, with "system" resolved. */
  theme: "dark" | "light";
  /** Whether terminals are read for screen readers, with "auto" resolved. */
  screenReader: boolean;
  /** Applies `settings` and saves them, after `delay` ms without another change if given. */
  update(settings: Settings, delay?: number): void;
  /** Saves a change still waiting for its delay. */
  flush(): void;
  /** Why the last change couldn't be saved (it was undone), until dismissed. */
  saveError: unknown;
  dismissSaveError(): void;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

const systemTheme = () => (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");

/** Applies the appearance to the document, where the CSS theme variables are selected. */
export function applyTheme(theme: "dark" | "light") {
  document.documentElement.dataset.theme = theme;
}

/**
 * Matches the native window (title bar, background while resizing) to the appearance, after
 * `applyTheme`: the colors are the theme's. The window's theme also themes the native menus
 * and dialogs, and on Windows and Linux decides the web view's `prefers-color-scheme`. The
 * first frame, before this runs, takes its background from the inline style in `index.html`.
 */
function applyWindowTheme(appearance: Appearance, theme: "dark" | "light") {
  const window = getCurrentWindow();
  const color = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  // null follows the system, so the title bar keeps tracking it in "system" mode. Through the
  // backend, which on Linux reads the desktop's color scheme (see `window_set_theme`).
  invoke("window_set_theme", { theme: appearance === "system" ? null : theme }).catch(console.error);
  window.setBackgroundColor(color("--bg")).catch(console.error);
  // Windows: the border around a snapped window, which blends in with the title bar.
  if (isWindows) {
    invoke("window_set_snapped_border_color", { color: color("--bg-sidebar") }).catch(console.error);
  }
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [system, setSystem] = useState<"dark" | "light">(systemTheme);

  useEffect(() => {
    // With the desktop's monospace font (Linux), which terminals default to.
    const font = invoke<string | null>("system_monospace_font").catch(() => null);
    const stored = invoke<Settings>("settings_get").catch((e) => {
      console.error(e);
      return DEFAULT_SETTINGS;
    });
    void Promise.all([stored, font]).then(([stored, font]) => {
      if (font) setSystemFont(font);
      setSettings(stored);
    });
    const query = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = () => setSystem(systemTheme());
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const loaded = settings !== null;
  const appearance = settings?.appearance ?? "system";
  const theme = appearance === "system" ? system : appearance;
  // Once the stored settings load: "system" before then would turn the window to the
  // system's appearance for a moment, where another one is chosen.
  useEffect(() => {
    if (!loaded) return;
    applyTheme(theme);
    applyWindowTheme(appearance, theme);
  }, [loaded, appearance, theme]);

  // Changed here or in another window; the page started in the stored one.
  const language = settings?.language;
  useEffect(() => {
    if (language !== undefined) followLanguageSetting(language).catch(console.error);
  }, [language]);

  // Before painting, so the first frame already has the text size.
  const textSize = settings?.textSize ?? "normal";
  useLayoutEffect(() => {
    document.documentElement.style.setProperty("--text-scale", String(TEXT_SCALES[textSize] ?? 1));
  }, [textSize]);

  // The app font before the default ones (see styles.css), also before painting. A list the
  // page can't read (a stray quote) is left out: it would take the default ones with it. It is
  // checked as the start of a list, where a value valid alone (`inherit`) may not be.
  const appFont = settings?.appFont ?? "";
  useLayoutEffect(() => {
    const style = document.documentElement.style;
    if (appFont && CSS.supports("font-family", `${appFont}, sans-serif`)) style.setProperty("--app-font", `${appFont},`);
    else style.removeProperty("--app-font");
  }, [appFont]);

  // Whether the system's screen reader is running, which the web view can't tell. An answer
  // to the question that comes after a change was told is out of date.
  const [screenReaderRunning, setScreenReaderRunning] = useState(false);
  const screenReaderTold = useRef(false);
  useWindowEvent<boolean>("screen-reader-changed", (running) => {
    screenReaderTold.current = true;
    setScreenReaderRunning(running);
  });
  useEffect(() => {
    invoke<boolean>("screen_reader_running").then((running) => {
      if (!screenReaderTold.current) setScreenReaderRunning(running);
    }, console.error);
  }, []);

  // Applied immediately; the backend's validated copy replaces it once saved (see
  // `latestSave`).
  const [saveError, setSaveError] = useState<unknown>(null);
  const saver = useMemo(
    () =>
      latestSave<Settings>({
        show: setSettings,
        store: (settings) => invoke<Settings>("settings_set", { settings }),
        load: () => invoke<Settings>("settings_get"),
        failed: (e) => setSaveError(() => e),
      }),
    [],
  );
  useConfigChanged("settings", saver.reload);

  // Wait for the stored settings so terminals start with the right theme and font.
  if (!settings) return null;
  const mode = settings.terminal.screenReaderMode;
  const screenReader = mode === "on" || (mode === "auto" && screenReaderRunning);
  const value = {
    settings,
    theme,
    screenReader,
    update: saver.save,
    flush: saver.flush,
    saveError,
    dismissSaveError: () => setSaveError(null),
  };
  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

/** Whether a media query (a system preference such as more contrast) matches, as it changes. */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const list = window.matchMedia(query);
    const onChange = () => setMatches(list.matches);
    onChange();
    list.addEventListener("change", onChange);
    return () => list.removeEventListener("change", onChange);
  }, [query]);
  return matches;
}

export function useSettings(): SettingsContextValue {
  const value = useContext(SettingsContext);
  if (!value) throw new Error("useSettings outside SettingsProvider");
  return value;
}
