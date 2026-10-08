import type { ITheme } from "@xterm/xterm";

import type { ProfileAppearance } from "./api";

export interface TerminalScheme {
  id: string;
  /** Proper name of the scheme; null for the defaults, whose names are translated. */
  name: string | null;
  dark: boolean;
  theme: ITheme;
}

/** Font stack used after the user's preferred font, covering macOS, Windows and CJK. */
export const DEFAULT_FONT_STACK = 'Menlo, "Cascadia Mono", Consolas, "PingFang SC", "Microsoft YaHei", monospace';

export const fontStack = (preferred: string) => (preferred ? `${preferred}, ${DEFAULT_FONT_STACK}` : DEFAULT_FONT_STACK);

const solarizedAnsi = {
  black: "#073642",
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
  white: "#eee8d5",
  brightBlack: "#002b36",
  brightRed: "#cb4b16",
  brightGreen: "#586e75",
  brightYellow: "#657b83",
  brightBlue: "#839496",
  brightMagenta: "#6c71c4",
  brightCyan: "#93a1a1",
  brightWhite: "#fdf6e3",
};

export const TERMINAL_SCHEMES: TerminalScheme[] = [
  {
    id: "default-dark",
    name: null,
    dark: true,
    theme: {
      background: "#1e1e1e",
      foreground: "#cccccc",
      cursor: "#aeafad",
      selectionBackground: "#264f78",
      black: "#000000",
      red: "#cd3131",
      green: "#0dbc79",
      yellow: "#e5e510",
      blue: "#2472c8",
      magenta: "#bc3fbc",
      cyan: "#11a8cd",
      white: "#e5e5e5",
      brightBlack: "#666666",
      brightRed: "#f14c4c",
      brightGreen: "#23d18b",
      brightYellow: "#f5f543",
      brightBlue: "#3b8eea",
      brightMagenta: "#d670d6",
      brightCyan: "#29b8db",
      brightWhite: "#e5e5e5",
    },
  },
  {
    id: "default-light",
    name: null,
    dark: false,
    theme: {
      background: "#ffffff",
      foreground: "#333333",
      cursor: "#333333",
      selectionBackground: "#add6ff",
      black: "#000000",
      red: "#cd3131",
      green: "#00bc00",
      yellow: "#949800",
      blue: "#0451a5",
      magenta: "#bc05bc",
      cyan: "#0598bc",
      white: "#555555",
      brightBlack: "#666666",
      brightRed: "#cd3131",
      brightGreen: "#14ce14",
      brightYellow: "#b5ba00",
      brightBlue: "#0451a5",
      brightMagenta: "#bc05bc",
      brightCyan: "#0598bc",
      brightWhite: "#a5a5a5",
    },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    dark: true,
    theme: { background: "#002b36", foreground: "#839496", cursor: "#93a1a1", selectionBackground: "#0a4a5a", ...solarizedAnsi },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    dark: false,
    theme: { background: "#fdf6e3", foreground: "#657b83", cursor: "#586e75", selectionBackground: "#e6dfc8", ...solarizedAnsi },
  },
  {
    id: "dracula",
    name: "Dracula",
    dark: true,
    theme: {
      background: "#282a36",
      foreground: "#f8f8f2",
      cursor: "#f8f8f2",
      selectionBackground: "#44475a",
      black: "#21222c",
      red: "#ff5555",
      green: "#50fa7b",
      yellow: "#f1fa8c",
      blue: "#bd93f9",
      magenta: "#ff79c6",
      cyan: "#8be9fd",
      white: "#f8f8f2",
      brightBlack: "#6272a4",
      brightRed: "#ff6e6e",
      brightGreen: "#69ff94",
      brightYellow: "#ffffa5",
      brightBlue: "#d6acff",
      brightMagenta: "#ff92df",
      brightCyan: "#a4ffff",
      brightWhite: "#ffffff",
    },
  },
  {
    id: "one-dark",
    name: "One Dark",
    dark: true,
    theme: {
      background: "#282c34",
      foreground: "#abb2bf",
      cursor: "#528bff",
      selectionBackground: "#3e4451",
      black: "#3f4451",
      red: "#e06c75",
      green: "#98c379",
      yellow: "#e5c07b",
      blue: "#61afef",
      magenta: "#c678dd",
      cyan: "#56b6c2",
      white: "#d7dae0",
      brightBlack: "#4f5666",
      brightRed: "#be5046",
      brightGreen: "#98c379",
      brightYellow: "#d19a66",
      brightBlue: "#61afef",
      brightMagenta: "#c678dd",
      brightCyan: "#56b6c2",
      brightWhite: "#e6e6e6",
    },
  },
  {
    id: "nord",
    name: "Nord",
    dark: true,
    theme: {
      background: "#2e3440",
      foreground: "#d8dee9",
      cursor: "#d8dee9",
      selectionBackground: "#434c5e",
      black: "#3b4252",
      red: "#bf616a",
      green: "#a3be8c",
      yellow: "#ebcb8b",
      blue: "#81a1c1",
      magenta: "#b48ead",
      cyan: "#88c0d0",
      white: "#e5e9f0",
      brightBlack: "#4c566a",
      brightRed: "#bf616a",
      brightGreen: "#a3be8c",
      brightYellow: "#ebcb8b",
      brightBlue: "#81a1c1",
      brightMagenta: "#b48ead",
      brightCyan: "#8fbcbb",
      brightWhite: "#eceff4",
    },
  },
  {
    id: "github-light",
    name: "GitHub Light",
    dark: false,
    theme: {
      background: "#ffffff",
      foreground: "#1f2328",
      cursor: "#0969da",
      selectionBackground: "#b6e3ff",
      black: "#24292f",
      red: "#cf222e",
      green: "#116329",
      yellow: "#4d2d00",
      blue: "#0969da",
      magenta: "#8250df",
      cyan: "#1b7c83",
      white: "#6e7781",
      brightBlack: "#57606a",
      brightRed: "#a40e26",
      brightGreen: "#1a7f37",
      brightYellow: "#633c01",
      brightBlue: "#218bff",
      brightMagenta: "#a475f9",
      brightCyan: "#3192aa",
      brightWhite: "#8c959f",
    },
  },
];

/** The scheme for a setting value; "auto" (or an unknown id) follows the app appearance. */
export function resolveScheme(id: string, appearance: "dark" | "light"): TerminalScheme {
  const found = TERMINAL_SCHEMES.find((s) => s.id === id);
  return found ?? TERMINAL_SCHEMES.find((s) => s.id === `default-${appearance}`)!;
}

/**
 * The scheme of a session's terminal: the session's own choice over the setting's, with its
 * background color if it has one (see `ProfileAppearance`).
 */
export function sessionScheme(
  settingId: string,
  appearance: "dark" | "light",
  own: ProfileAppearance | undefined,
): TerminalScheme {
  const scheme = resolveScheme(own?.colorScheme ?? settingId, appearance);
  const background = own?.background;
  if (!background) return scheme;
  return { ...scheme, dark: isDark(background), theme: { ...scheme.theme, background } };
}

/**
 * A tab's mark for a session background color: the same hue at a middle lightness, so dark
 * and light backgrounds alike stand out against the tab bar.
 */
export function tabMark(color: string): string {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(color.slice(i, i + 2), 16) / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  let hue = 0;
  if (d > 0) {
    if (max === r) hue = ((g - b) / d) % 6;
    else if (max === g) hue = (b - r) / d + 2;
    else hue = (r - g) / d + 4;
  }
  const lightness = (max + min) / 2;
  const saturation = d === 0 ? 0 : d / (1 - Math.abs(2 * lightness - 1));
  return `hsl(${Math.round(hue * 60 + 360) % 360} ${Math.round(Math.max(saturation, d > 0 ? 0.5 : 0) * 100)}% 55%)`;
}

/** Whether a #rrggbb color is dark (relative luminance, as WCAG defines it). */
function isDark(color: string): boolean {
  const channel = (i: number) => {
    const c = parseInt(color.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5) < 0.18;
}

/** Search highlight colors that read well on the scheme's background (#RRGGBB only). */
export const searchDecorations = (scheme: TerminalScheme) =>
  scheme.dark
    ? {
        matchBackground: "#4b3d12",
        matchOverviewRuler: "#d1a73a",
        activeMatchBackground: "#a8761a",
        activeMatchColorOverviewRuler: "#f2c14e",
      }
    : {
        matchBackground: "#fff1a8",
        matchOverviewRuler: "#d4a600",
        activeMatchBackground: "#f5c542",
        activeMatchColorOverviewRuler: "#b98900",
      };
