import { describe, expect, it } from "vitest";

import { TabMoves } from "./tabMoves";

describe("TabMoves", () => {
  it("is over once every session has moved or stayed", () => {
    const moves = new TabMoves();
    moves.start(1, "window-1", [10, 11, 12], [10, 11]);
    expect(moves.has(1)).toBe(true);
    expect(moves.started(1)).toEqual([]);
    expect(moves.resolve(10, false)).toEqual([]);
    // Not one of its panes.
    expect(moves.resolve(99, true)).toEqual([]);
    expect(moves.resolve(11, true)).toEqual([{ key: 1, gone: [10, 12] }]);
    expect(moves.has(1)).toBe(false);
    // Heard again: nothing more.
    expect(moves.resolve(11, true)).toEqual([]);
  });

  it("is over at once without sessions", () => {
    const moves = new TabMoves();
    moves.start(1, "window-1", [10], []);
    expect(moves.started(1)).toEqual([{ key: 1, gone: [10] }]);
    expect(moves.has(1)).toBe(false);
  });

  it("keeps the sessions a window that closed hadn't taken", () => {
    const moves = new TabMoves();
    moves.start(1, null, [10, 11], [10, 11]);
    moves.start(2, "window-2", [20], [20]);
    moves.resolve(10, false);
    expect(moves.windowClosed("window-1")).toEqual([]);
    moves.opened(1, "window-3");
    expect(moves.windowClosed("window-3")).toEqual([{ key: 1, gone: [10] }]);
    expect(moves.has(2)).toBe(true);
  });

  it("keeps the sessions of a new window that closed before it was known to be the one", () => {
    const moves = new TabMoves();
    moves.start(1, null, [10], [10]);
    expect(moves.windowClosed("window-3")).toEqual([]);
    expect(moves.opened(1, "window-3")).toEqual([{ key: 1, gone: [] }]);
    expect(moves.has(1)).toBe(false);
  });

  it("leaves the panes of the tab that didn't move", () => {
    const moves = new TabMoves();
    // Pane 11 of tab 1, which also has 10 and 12.
    moves.start(1, "window-1", [11], [11]);
    expect(moves.resolve(10, false)).toEqual([]);
    expect(moves.resolve(11, false)).toEqual([{ key: 1, gone: [11] }]);
  });

  it("forgets a move that failed to start", () => {
    const moves = new TabMoves();
    moves.start(1, "window-1", [10], [10]);
    moves.cancel(1);
    expect(moves.has(1)).toBe(false);
    expect(moves.resolve(10, false)).toEqual([]);
  });
});
