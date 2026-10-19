import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { followLanguageSetting } from "../i18n/index";
import { appSettings, DEFAULT_SETTINGS } from "../ipc/api";
import type { Appearance, Settings, TextSize } from "../ipc/bindings";
import { windows } from "../ipc/windows";
import { isWindows } from "../lib/platform";
import { settingsStore } from "../lib/settings";
import { setSystemFont, systemFontFamilies } from "../lib/terminalSchemes";
import { useConfigChanged, useWindowEvent } from "./windowEvents";

export type {
  Appearance,
  CursorStyle,
  FileSettings,
  Ligatures,
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
} from "../ipc/bindings";

const TEXT_SCALES: Record<TextSize, number> = { normal: 1, large: 1.15, larger: 1.3 };

/** How long a setting typed into a field waits for the next key before it is saved. */
export const TYPING_SAVE_DELAY = 500;

interface SettingsContextValue {
  settings: Settings;
  /** The appearance in effect, with "system" resolved. */
  theme: "dark" | "light";
  /** Whether terminals are read for screen readers, with "auto" resolved. */
  screenReader: boolean;
  /**
   * Applies `change` to the settings and saves them, after `delay` ms without another change if
   * given. A change rather than the settings it makes, so that a change another window saves
   * meanwhile is kept too (see `latestSave`).
   */
  update(change: (settings: Settings) => Settings, delay?: number): void;
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
  // backend, which on Linux reads the desktop's color scheme (see `window::set_theme`).
  windows.setTheme(appearance === "system" ? null : theme);
  window.setBackgroundColor(color("--bg")).catch(console.error);
  // Windows: the border around a snapped window, which blends in with the title bar.
  if (isWindows) {
    windows.setSnappedBorderColor(color("--bg-sidebar"));
  }
}

/**
 * Puts a font list the user set before the default ones of a stack (`var(--name,) …`), with its
 * comma. A list the page can't read is left out: it would take the default ones with it. It is
 * checked as the start of a list, where a value valid alone (`inherit`) may not be, and before a
 * line break, which ends a string left open by a stray quote: `CSS.supports` closes it at the end
 * of the value, so the rest of the stack would read as part of the name.
 */
function setFontList(name: string, fonts: string) {
  const style = document.documentElement.style;
  if (fonts && CSS.supports("font-family", `${fonts}\n, sans-serif`)) style.setProperty(name, `${fonts},`);
  else style.removeProperty(name);
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  // Applied immediately; the backend's validated copy replaces it once saved (see
  // `latestSave`).
  const { value: settings, saveError } = useSyncExternalStore(settingsStore.subscribe, settingsStore.get);
  const [system, setSystem] = useState<"dark" | "light">(systemTheme);

  useEffect(() => {
    // With the desktop's monospace font (Linux), which terminals default to.
    const font = appSettings.systemMonospaceFont().catch(() => null);
    const stored = appSettings.get().catch((e) => {
      console.error(e);
      return DEFAULT_SETTINGS;
    });
    void Promise.all([stored, font]).then(([stored, font]) => {
      if (font) setSystemFont(font);
      settingsStore.loaded(stored);
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

  // The app font before the default ones (see styles/theme.css), also before painting.
  const appFont = settings?.appFont ?? "";
  useLayoutEffect(() => setFontList("--app-font", appFont), [appFont]);

  // The terminal font before the interface's monospace ones (`--font-mono`), so commands and
  // paths look as in the terminal; on Linux, without one, the desktop's monospace font as there.
  // Only the setting's: a session's own font is not the interface's. The desktop's font is known
  // once the settings have loaded.
  const terminalFont = settings?.terminal.fontFamily ?? "";
  useLayoutEffect(() => setFontList("--terminal-font", terminalFont || (loaded ? systemFontFamilies() : "")), [terminalFont, loaded]);

  // Whether the system's screen reader is running, which the web view can't tell. An answer
  // to the question that comes after a change was told is out of date.
  const [screenReaderRunning, setScreenReaderRunning] = useState(false);
  const screenReaderTold = useRef(false);
  useWindowEvent("screen-reader-changed", (running) => {
    screenReaderTold.current = true;
    setScreenReaderRunning(running);
  });
  useEffect(() => {
    appSettings.screenReaderRunning().then((running) => {
      if (!screenReaderTold.current) setScreenReaderRunning(running);
    }, console.error);
  }, []);

  useConfigChanged("settings", settingsStore.reload);

  // Wait for the stored settings so terminals start with the right theme and font.
  if (!settings) return null;
  const mode = settings.terminal.screenReaderMode;
  const screenReader = mode === "on" || (mode === "auto" && screenReaderRunning);
  const value = {
    settings,
    theme,
    screenReader,
    update: settingsStore.save,
    flush: settingsStore.flush,
    saveError,
    dismissSaveError: settingsStore.dismissSaveError,
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
