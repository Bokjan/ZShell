import { describe, expect, it } from "vitest";

import { ReadBack, type ReadBackChange, type ReadBackTerminal } from "./readBack";

/** A terminal that keeps its lines as xterm.js does, and reports what each scroll event was. */
class FakeTerminal implements ReadBackTerminal {
  cols = 80;
  options: { scrollback: number };
  type: "normal" | "alternate" = "normal";
  lines: { isWrapped: boolean }[] = [];
  baseY = 0;
  viewportY = 0;
  cursorY: number;
  markers: { line: number; isDisposed: boolean; dispose(): void }[] = [];
  changes: ReadBackChange[] = [];
  readBack: ReadBack;

  constructor(
    public rows: number,
    scrollback: number,
  ) {
    this.options = { scrollback };
    for (let i = 0; i < rows; i++) this.lines.push({ isWrapped: false });
    this.cursorY = rows - 1;
    this.readBack = new ReadBack(this);
  }

  get buffer() {
    const term = this;
    return {
      active: {
        get type() {
          return term.type;
        },
        get baseY() {
          return term.type === "alternate" ? 0 : term.baseY;
        },
        get viewportY() {
          return term.type === "alternate" ? 0 : term.viewportY;
        },
        get cursorY() {
          return term.cursorY;
        },
        get length() {
          return term.type === "alternate" ? term.rows : term.lines.length;
        },
        getLine: (y: number) => term.lines[y],
      },
    };
  }

  registerMarker(offset = 0) {
    const marker = {
      line: this.baseY + this.cursorY + offset,
      isDisposed: false,
      dispose: () => {
        marker.isDisposed = true;
        this.markers = this.markers.filter((other) => other !== marker);
      },
    };
    this.markers.push(marker);
    return marker;
  }

  /** Lines of output, each scrolling the screen up, as the cursor is on its last line. */
  output(count: number, wrapped = false) {
    for (let i = 0; i < count; i++) {
      const away = this.viewportY !== this.baseY;
      this.lines.push({ isWrapped: wrapped });
      if (this.lines.length > this.rows + this.options.scrollback) {
        this.lines.shift();
        for (const marker of [...this.markers]) {
          marker.line--;
          if (marker.line < 0) {
            marker.line = -1;
            marker.dispose();
          }
        }
        if (away) this.viewportY = Math.max(this.viewportY - 1, 0);
        else this.viewportY = this.baseY;
      } else {
        this.baseY++;
        if (!away) this.viewportY = this.baseY;
      }
      this.scrolled();
    }
  }

  /** A scrolling region scrolls: xterm.js fires the event, but adds nothing to the scrollback. */
  scrollRegion() {
    this.scrolled();
  }

  scrollTo(y: number) {
    this.viewportY = Math.max(0, Math.min(y, this.baseY));
    this.scrolled();
  }

  resize(cols: number, rows: number) {
    this.cols = cols;
    // Taller with no scrollback to pull down: blank lines at the bottom.
    for (let i = this.rows; i < rows; i++) this.lines.push({ isWrapped: false });
    this.rows = rows;
    this.readBack.resize();
  }

  private scrolled() {
    this.changes.push(this.readBack.scrolled());
  }

  get last() {
    return this.changes[this.changes.length - 1];
  }
}

describe("ReadBack", () => {
  it("counts nothing for output at the end, which doesn't move the view", () => {
    const term = new FakeTerminal(5, 100);
    term.output(10);
    expect(term.readBack.newLines).toBe(0);
    expect(term.changes.every((change) => !change.moved && !change.trimmingView)).toBe(true);
  });

  it("counts the lines that arrive after the view leaves the end, until it comes back", () => {
    const term = new FakeTerminal(5, 100);
    term.output(20);
    term.scrollTo(3);
    expect(term.last.moved).toBe(true);
    term.output(4);
    expect(term.readBack.newLines).toBe(4);
    expect(term.viewportY).toBe(3);
    expect(term.last.moved).toBe(false);
    term.scrollTo(2);
    expect(term.last.moved).toBe(true);
    expect(term.readBack.newLines).toBe(4);
    term.scrollTo(term.baseY);
    expect(term.last.moved).toBe(true);
    expect(term.readBack.newLines).toBe(0);
    term.scrollTo(0);
    term.output(1);
    expect(term.readBack.newLines).toBe(1);
  });

  it("counts nothing for a scrolling region, which adds no lines", () => {
    const term = new FakeTerminal(5, 100);
    term.output(20);
    term.scrollTo(3);
    term.scrollRegion();
    expect(term.readBack.newLines).toBe(0);
    expect(term.last).toEqual({ moved: false, trimmingView: false });
  });

  it("counts a long line the terminal wraps once", () => {
    const term = new FakeTerminal(5, 100);
    term.output(20);
    term.scrollTo(3);
    term.output(1);
    term.output(2, true);
    term.output(1);
    expect(term.readBack.newLines).toBe(2);
  });

  it("keeps counting as a full scrollback is trimmed, and tells once when output removes lines in view", () => {
    const term = new FakeTerminal(5, 10);
    term.output(30);
    term.scrollTo(2);
    term.output(1);
    expect(term.viewportY).toBe(1);
    expect(term.last).toEqual({ moved: false, trimmingView: false });
    // From line 1 to 0, the lines in view stay.
    term.output(1);
    expect(term.viewportY).toBe(0);
    expect(term.last.trimmingView).toBe(false);
    term.output(1);
    expect(term.last.trimmingView).toBe(true);
    term.output(30);
    expect(term.changes.filter((change) => change.trimmingView)).toHaveLength(1);
    expect(term.readBack.newLines).toBe(33);
    // Again after the view came back to the end and left it once more.
    term.scrollTo(term.baseY);
    term.scrollTo(0);
    term.output(1);
    expect(term.last.trimmingView).toBe(true);
  });

  it("keeps counting when more lines arrive than the scrollback holds", () => {
    const term = new FakeTerminal(5, 3);
    term.output(10);
    term.scrollTo(0);
    term.output(12);
    expect(term.readBack.newLines).toBe(12);
  });

  it("stays at the end on the alternate screen", () => {
    const term = new FakeTerminal(5, 100);
    term.output(20);
    term.scrollTo(3);
    term.output(2);
    term.type = "alternate";
    term.scrollRegion();
    expect(term.readBack.newLines).toBe(0);
  });

  it("doesn't count the lines a resize adds or wraps again", () => {
    const term = new FakeTerminal(5, 100);
    term.output(20);
    term.scrollTo(3);
    term.output(2);
    term.resize(60, 8);
    term.output(1);
    expect(term.readBack.newLines).toBe(3);
  });

  it("keeps one marker while the view is away, and none at the end", () => {
    const term = new FakeTerminal(5, 10);
    term.output(30);
    expect(term.markers).toHaveLength(0);
    term.scrollTo(3);
    term.output(25);
    expect(term.markers).toHaveLength(1);
    term.scrollTo(term.baseY);
    expect(term.markers).toHaveLength(0);
  });

  it("tells its listeners when the count changes, and only then", () => {
    const term = new FakeTerminal(5, 100);
    let calls = 0;
    const stop = term.readBack.subscribe(() => calls++);
    term.output(20);
    term.scrollTo(3);
    expect(calls).toBe(0);
    term.output(2);
    term.scrollRegion();
    expect(calls).toBe(2);
    term.scrollTo(term.baseY);
    expect(calls).toBe(3);
    stop();
    term.scrollTo(0);
    term.output(1);
    expect(calls).toBe(3);
  });
});
