import type { Terminal } from "@xterm/xterm";

/** The parts of xterm.js's internals read here, any of which may be missing in another version. */
interface Internals {
  _core?: {
    coreMouseService?: { activeEncoding?: string };
    coreService?: {
      isCursorHidden?: boolean;
      decPrivateModes?: { cursorStyle?: "block" | "underline" | "bar"; cursorBlink?: boolean };
    };
    buffer?: { scrollTop?: number; scrollBottom?: number };
  };
}

/** DECSCUSR's blinking shape for each cursor style; the steady one is the next number. */
const CURSOR_SHAPES = { block: 1, underline: 3, bar: 5 };

/**
 * What a program set in the terminal that the serialize addon doesn't keep, as escape
 * sequences to write after what it serialized: the mouse reports' encoding (SGR), a hidden
 * cursor, the cursor's shape, and the scrolling region, which full screen programs (vim,
 * tmux, htop) set once and expect to stay. xterm.js has no public API for them, so they are
 * read from its internals, and left out if those change.
 */
export function extraModes(term: Terminal): string {
  const core = (term as unknown as Internals)._core;
  if (!core) return "";
  let modes = "";
  const encoding = core.coreMouseService?.activeEncoding;
  if (encoding === "SGR") modes += "\x1b[?1006h";
  else if (encoding === "SGR_PIXELS") modes += "\x1b[?1016h";
  const { isCursorHidden, decPrivateModes } = core.coreService ?? {};
  if (isCursorHidden) modes += "\x1b[?25l";
  const style = decPrivateModes?.cursorStyle;
  if (style && style in CURSOR_SHAPES) modes += `\x1b[${CURSOR_SHAPES[style] + (decPrivateModes?.cursorBlink ? 0 : 1)} q`;
  const { scrollTop, scrollBottom } = core.buffer ?? {};
  // Setting it moves the cursor home: saved and restored around it.
  if (scrollTop !== undefined && scrollBottom !== undefined && (scrollTop !== 0 || scrollBottom !== term.rows - 1)) {
    modes += `\x1b7\x1b[${scrollTop + 1};${scrollBottom + 1}r\x1b8`;
  }
  return modes;
}
