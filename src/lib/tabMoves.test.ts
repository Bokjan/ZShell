import { describe, expect, it } from "vitest";

import { TabMoves } from "./tabMoves";

describe("TabMoves", () => {
  it("is over once every session has moved or stayed", () => {
    const moves = new TabMoves();
    moves.start(1, "window-1", [10, 11]);
    expect(moves.has(1)).toBe(true);
    expect(moves.started(1)).toEqual([]);
    expect(moves.resolve(10, false)).toEqual([]);
    // Not one of its panes.
    expect(moves.resolve(99, true)).toEqual([]);
    expect(moves.resolve(11, true)).toEqual([{ key: 1, stayed: new Set([11]) }]);
    expect(moves.has(1)).toBe(false);
    // Heard again: nothing more.
    expect(moves.resolve(11, true)).toEqual([]);
  });

  it("is over at once without sessions", () => {
    const moves = new TabMoves();
    moves.start(1, "window-1", []);
    expect(moves.started(1)).toEqual([{ key: 1, stayed: new Set() }]);
    expect(moves.has(1)).toBe(false);
  });

  it("keeps the sessions a window that closed hadn't taken", () => {
    const moves = new TabMoves();
    moves.start(1, null, [10, 11]);
    moves.start(2, "window-2", [20]);
    moves.resolve(10, false);
    // Not yet known to be its window.
    expect(moves.windowClosed("window-3")).toEqual([]);
    moves.opened(1, "window-3");
    expect(moves.windowClosed("window-3")).toEqual([{ key: 1, stayed: new Set([11]) }]);
    expect(moves.has(2)).toBe(true);
  });

  it("forgets a move that failed to start", () => {
    const moves = new TabMoves();
    moves.start(1, "window-1", [10]);
    moves.cancel(1);
    expect(moves.has(1)).toBe(false);
    expect(moves.resolve(10, false)).toEqual([]);
  });
});
