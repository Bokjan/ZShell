import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { isWindows } from "./platform";

export type Appearance = "system" | "dark" | "light";
/** The size of the interface's text; the terminal has its own font size. */
export type TextSize = "normal" | "large" | "larger";
const TEXT_SCALES: Record<TextSize, number> = { normal: 1, large: 1.15, larger: 1.3 };
export type CursorStyle = "block" | "bar" | "underline";
/** What right-clicking the terminal does. */
export type RightClick = "menu" | "paste";

/** The terminal font sizes allowed, in the settings and in sessions. */
export const FONT_SIZE_MIN = 6;
export const FONT_SIZE_MAX = 48;

export interface TerminalSettings {
  /** A scheme id from terminalSchemes.ts, or "auto" to follow the appearance. */
  colorScheme: string;
  /** Preferred font family; empty uses the default stack. */
  fontFamily: string;
  fontSize: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  scrollback: number;
  copyOnSelect: boolean;
  rightClick: RightClick;
  /** Ask before pasting text with line breaks while the shell would run each line. */
  confirmMultilinePaste: boolean;
  /** macOS only: Option sends Meta (Esc-prefixed) sequences. */
  optionAsMeta: boolean;
  /** Screen readers can read the terminal and hear new output. */
  screenReader: boolean;
}

export interface TabSettings {
  /** Show the title set by the shell instead of the session name. */
  followRemoteTitle: boolean;
  /** Ask before closing tabs that are connected or running a program. */
  confirmClose: boolean;
}

export interface SidebarSettings {
  /** Show the most recently opened sessions above the list. */
  showRecent: boolean;
}

export interface FileSettings {
  /** Where downloads go without asking; empty for the Downloads folder. */
  downloadDirectory: string;
  /** The application remote files are edited with; empty for the system's default. */
  editor: string;
}

/** What happens when `sz` sends files: asked, saved into the download folder, or a folder chosen each time. */
export type ZmodemReceive = "ask" | "downloads" | "chooseFolder";

export interface ZmodemSettings {
  receive: ZmodemReceive;
}

export type LogFormat = "text" | "raw";

export interface LogSettings {
  /** Empty for the default, `ZShellLogs` in Documents. */
  directory: string;
  /** With `{session}`, `{host}`, `{user}`, `{date}` and `{time}`. */
  fileName: string;
  format: LogFormat;
  /** Start each line with its time (plain text only). */
  timestamps: boolean;
  /** Record local terminals from the start. */
  autoLocal: boolean;
  /** Delete logs older than this many days; 0 keeps them. */
  keepDays: number;
}

export const DEFAULT_LOG_FILE_NAME = "{session}_{date}_{time}.log";

export interface Settings {
  appearance: Appearance;
  textSize: TextSize;
  terminal: TerminalSettings;
  tabs: TabSettings;
  sidebar: SidebarSettings;
  files: FileSettings;
  zmodem: ZmodemSettings;
  logs: LogSettings;
}

export const DEFAULT_SETTINGS: Settings = {
  appearance: "system",
  textSize: "normal",
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
    screenReader: false,
  },
  tabs: { followRemoteTitle: true, confirmClose: true },
  sidebar: { showRecent: true },
  files: { downloadDirectory: "", editor: "" },
  zmodem: { receive: "ask" },
  logs: { directory: "", fileName: DEFAULT_LOG_FILE_NAME, format: "text", timestamps: false, autoLocal: false, keepDays: 0 },
};

interface SettingsContextValue {
  settings: Settings;
  /** The appearance in effect, with "system" resolved. */
  theme: "dark" | "light";
  update(settings: Settings): void;
}

const SettingsContext = createContext<SettingsContextValue | null>(null);

const systemTheme = () => (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");

/** Applies the appearance to the document, where the CSS theme variables are selected. */
export function applyTheme(theme: "dark" | "light") {
  document.documentElement.dataset.theme = theme;
}

/** Matches the native window (title bar, background while resizing) to the appearance. */
function applyWindowTheme(appearance: Appearance, theme: "dark" | "light") {
  const window = getCurrentWindow();
  // null follows the system, so the title bar keeps tracking it in "system" mode.
  window.setTheme(appearance === "system" ? null : theme).catch(console.error);
  window.setBackgroundColor(theme === "light" ? "#ffffff" : "#1e1e1e").catch(console.error);
  document.documentElement.style.background = theme === "light" ? "#ffffff" : "#1e1e1e";
  // Windows: the border around a snapped window, which blends in with the title bar.
  if (isWindows) {
    const color = getComputedStyle(document.documentElement).getPropertyValue("--bg-sidebar").trim();
    invoke("window_set_snapped_border_color", { color }).catch(console.error);
  }
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [system, setSystem] = useState<"dark" | "light">(systemTheme);

  useEffect(() => {
    invoke<Settings>("settings_get").then(setSettings, (e) => {
      console.error(e);
      setSettings(DEFAULT_SETTINGS);
    });
    const query = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = () => setSystem(systemTheme());
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const appearance = settings?.appearance ?? "system";
  const theme = appearance === "system" ? system : appearance;
  useEffect(() => {
    applyTheme(theme);
    applyWindowTheme(appearance, theme);
  }, [appearance, theme]);

  // Before painting, so the first frame already has the text size.
  const textSize = settings?.textSize ?? "normal";
  useLayoutEffect(() => {
    document.documentElement.style.setProperty("--text-scale", String(TEXT_SCALES[textSize] ?? 1));
  }, [textSize]);

  // Applied immediately; the backend's validated copy replaces it once saved, unless a
  // newer change was made in the meantime.
  const latest = useRef(0);
  const update = useCallback((next: Settings) => {
    const request = ++latest.current;
    setSettings(next);
    invoke<Settings>("settings_set", { settings: next }).then(
      (saved) => request === latest.current && setSettings(saved),
      console.error,
    );
  }, []);

  // Wait for the stored settings so terminals start with the right theme and font.
  if (!settings) return null;
  return <SettingsContext.Provider value={{ settings, theme, update }}>{children}</SettingsContext.Provider>;
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
