import { FitAddon } from "@xterm/addon-fit";
import type { ITerminalDimensions } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";

/** The parts of xterm.js's internals used here, any of which may be missing in another version. */
interface Internals {
  _core?: { _renderService?: { dimensions?: { css?: { cell?: { width: number; height: number } } }; clear?(): void } };
}

/** Lines of buffer from which a change of columns waits for the resizing to pause. */
export const DEBOUNCE_COLS_FROM = 200;
/** How long the resizing must pause for the columns to follow. */
export const COLS_DEBOUNCE_MS = 100;

/**
 * How a terminal follows its space as it is resized (the window's edge or a splitter dragged),
 * as VS Code's does: the rows change at once, and so do the columns while the buffer is short.
 * Each change of columns rewraps every line of the buffer, which with a long scrollback would
 * take most of each frame, so then the columns follow once the resizing pauses for
 * `COLS_DEBOUNCE_MS`. Returns the size to take now (none if it stays) and whether the columns
 * are left to follow.
 */
export function resizeStep(
  current: ITerminalDimensions,
  proposed: ITerminalDimensions,
  lines: number,
): { now: ITerminalDimensions | null; colsLater: boolean } {
  const colsLater = proposed.cols !== current.cols && lines >= DEBOUNCE_COLS_FROM;
  const now = { cols: colsLater ? current.cols : proposed.cols, rows: proposed.rows };
  return { now: now.cols === current.cols && now.rows === current.rows ? null : now, colsLater };
}

/** The addon's smallest terminal. */
const MIN_COLS = 2;
const MIN_ROWS = 1;

/** The columns and rows of cells that fit in a space, or nothing before the cells are measured. */
export function fitSize(
  space: { width: number; height: number },
  cell: { width: number; height: number },
): ITerminalDimensions | undefined {
  if (!(cell.width > 0 && cell.height > 0)) return undefined;
  return {
    cols: Math.max(MIN_COLS, Math.floor(Math.max(0, space.width) / cell.width)),
    rows: Math.max(MIN_ROWS, Math.floor(Math.max(0, space.height) / cell.height)),
  };
}

/**
 * The fit addon, with the text across the whole width. The addon keeps the scroll bar's width
 * (14px) free on the right, but xterm.js 6 draws its scroll bar over the text, as VS Code's
 * overlay one that fades out, so that strip would only stay empty. It measures the same things
 * as the addon, once per fit.
 */
export class FullWidthFitAddon extends FitAddon {
  private term: Terminal | undefined;

  override activate(term: Terminal): void {
    super.activate(term);
    this.term = term;
  }

  /** Resizes the terminal to `size` as `fit` does, drawing all of it again. */
  resizeTo(size: ITerminalDimensions): void {
    const term = this.term;
    if (!term || (term.cols === size.cols && term.rows === size.rows)) return;
    (term as unknown as Internals)._core?._renderService?.clear?.();
    term.resize(size.cols, size.rows);
  }

  override proposeDimensions(): ITerminalDimensions | undefined {
    const element = this.term?.element;
    const parent = element?.parentElement;
    const cell = (this.term as unknown as Internals | undefined)?._core?._renderService?.dimensions?.css?.cell;
    if (!element || !parent || !cell) return undefined;
    const outer = window.getComputedStyle(parent);
    const inner = window.getComputedStyle(element);
    const px = (style: CSSStyleDeclaration, name: string) => parseInt(style.getPropertyValue(name)) || 0;
    return fitSize(
      {
        width: px(outer, "width") - px(inner, "padding-left") - px(inner, "padding-right"),
        height: px(outer, "height") - px(inner, "padding-top") - px(inner, "padding-bottom"),
      },
      cell,
    );
  }
}
