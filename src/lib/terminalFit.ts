import { FitAddon } from "@xterm/addon-fit";
import type { ITerminalDimensions } from "@xterm/addon-fit";
import type { Terminal } from "@xterm/xterm";

/** The parts of xterm.js's internals read here, any of which may be missing in another version. */
interface Internals {
  _core?: { _renderService?: { dimensions?: { css?: { cell?: { width: number; height: number } } } } };
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
