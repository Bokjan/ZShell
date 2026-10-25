/** A line of the buffer, as xterm.js's `IBufferLine`. */
interface Line {
  /** Continues the line above it, which was too long for the terminal's width. */
  readonly isWrapped: boolean;
}

/** As xterm.js's `IMarker`: a line that keeps its place as lines are trimmed from the top. */
interface Marker {
  /** Where the line is now in the buffer, or -1 once it has been trimmed. */
  readonly line: number;
  readonly isDisposed: boolean;
  dispose(): void;
}

/** The parts of xterm.js's `Terminal` read here. */
export interface ReadBackTerminal {
  readonly cols: number;
  readonly rows: number;
  readonly options: { readonly scrollback?: number };
  readonly buffer: {
    readonly active: {
      readonly type: "normal" | "alternate";
      /** The first line of the screen, below the scrollback. */
      readonly baseY: number;
      /** The first line in view. */
      readonly viewportY: number;
      readonly cursorY: number;
      /** The lines of the scrollback and the screen together. */
      readonly length: number;
      getLine(y: number): Line | undefined;
    };
  };
  /** A marker on the cursor's line, or `offset` lines below it. */
  registerMarker(offset?: number): Marker;
}

/** What one of the terminal's scroll events was, for `TerminalView`. */
export interface ReadBackChange {
  /** The view moved through the lines: scrolled, or came to or left the end, rather than output arriving. */
  readonly moved: boolean;
  /** Output arrived and removed a line in view, for the first time since the view left the end. */
  readonly trimmingView: boolean;
}

/** Most events: output arriving at the end, or below the view. */
const UNCHANGED: ReadBackChange = { moved: false, trimmingView: false };

/**
 * Follows the view of a terminal while it reads back through the scrollback: the lines of
 * output that arrived below it since it left the end, and whether output has started to
 * remove the lines in view, which happens once the scrollback is full and the view has
 * reached its top. Fed every scroll event of the terminal, which xterm.js fires for each
 * line that output adds (even when the view stays where it is), for the view scrolling, and
 * for a scrolling region (vim, a status line) that adds no lines to the scrollback. Output
 * at the end is most of them, so that case does next to nothing; while the view is away,
 * each event reads the lines that came in, and one marker stays on the line counting started
 * from, rather than a marker for each line (each one listens to the buffer).
 *
 * Lines are counted as they come in at the bottom, so a program that draws over lines it
 * already wrote (a progress bar, Claude Code's prompt) adds none until it writes a new one;
 * a long line the terminal wraps counts once. Programs on the alternate screen have no
 * scrollback, and the view never leaves its end there.
 */
export class ReadBack {
  private lines = 0;
  private readonly listeners = new Set<() => void>();
  /** The last line when counting started, kept on it as the scrollback is trimmed. */
  private anchor: Marker | null = null;
  /** How many lines below `anchor` have been counted. */
  private scanned = 0;
  private viewportY = 0;
  private atEnd = true;
  /** Whether `trimmingView` was reported since the view left the end. */
  private warned = false;
  private cols: number;
  private rows: number;

  constructor(private readonly term: ReadBackTerminal) {
    this.cols = term.cols;
    this.rows = term.rows;
  }

  /** Output lines that arrived since the view left the end; 0 while it is there. */
  get newLines(): number {
    return this.lines;
  }

  /** Calls `listener` whenever `newLines` changes; returns the way to stop. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** On each of the terminal's scroll events. */
  scrolled(): ReadBackChange {
    const buffer = this.term.buffer.active;
    const viewportY = buffer.viewportY;
    const atEnd = viewportY === buffer.baseY || buffer.type === "alternate";
    const wasViewportY = this.viewportY;
    const wasAtEnd = this.atEnd;
    this.viewportY = viewportY;
    this.atEnd = atEnd;
    if (atEnd) {
      if (wasAtEnd) return UNCHANGED;
      this.reset();
      return { moved: viewportY !== wasViewportY, trimmingView: false };
    }
    let added = false;
    if (!this.anchor || this.resized()) this.markLast();
    else added = this.count();
    const trimmingView = added && wasViewportY === 0 && viewportY === 0 && !this.warned && this.full();
    if (trimmingView) this.warned = true;
    const moved = !added && viewportY !== wasViewportY;
    return moved || trimmingView ? { moved, trimmingView } : UNCHANGED;
  }

  /**
   * On each of the terminal's resizes, which wrap the lines again (merging or splitting them)
   * or add blank ones at the bottom: counting goes on from the new last line.
   */
  resize() {
    if (this.anchor && this.resized()) this.markLast();
  }

  dispose() {
    this.reset();
    this.listeners.clear();
  }

  /** Counts the lines below those counted; whether any came in. */
  private count(): boolean {
    const anchor = this.anchor!;
    const buffer = this.term.buffer.active;
    const last = buffer.length - 1;
    let lines = 0;
    if (anchor.isDisposed || anchor.line < 0) {
      // Trimmed away, with all the scrollback above it: nothing below the view is left to
      // tell where counting was, so the line that just came in counts alone.
      if (!buffer.getLine(last)?.isWrapped) lines++;
      this.markLast();
    } else {
      const from = anchor.line + this.scanned + 1;
      if (from > last) return false;
      for (let y = from; y <= last; y++) {
        if (!buffer.getLine(y)?.isWrapped) lines++;
      }
      this.scanned = last - anchor.line;
    }
    if (lines > 0) this.setLines(this.lines + lines);
    return true;
  }

  /** Whether the terminal has changed size since counting started, and takes the new size. */
  private resized(): boolean {
    const { cols, rows } = this.term;
    const changed = cols !== this.cols || rows !== this.rows;
    this.cols = cols;
    this.rows = rows;
    return changed;
  }

  private markLast() {
    const buffer = this.term.buffer.active;
    this.anchor?.dispose();
    this.anchor = this.term.registerMarker(buffer.length - 1 - (buffer.baseY + buffer.cursorY));
    this.scanned = 0;
  }

  /** Whether the scrollback is full, so that each line output adds removes the oldest. */
  private full(): boolean {
    const buffer = this.term.buffer.active;
    return buffer.length >= this.term.rows + (this.term.options.scrollback ?? 0);
  }

  private setLines(lines: number) {
    this.lines = lines;
    for (const listener of this.listeners) listener();
  }

  private reset() {
    this.anchor?.dispose();
    this.anchor = null;
    this.scanned = 0;
    this.warned = false;
    this.resized();
    if (this.lines !== 0) this.setLines(0);
  }
}
