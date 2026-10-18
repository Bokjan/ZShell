/**
 * Tabs moving out of this window to another (see `useWindows`), as plain state: a tab moving
 * out (or one of its panes) stays here until each of its panes' sessions has moved, or stayed
 * because the other window couldn't take it (it says so, or closes first). Then the move is
 * over, and the panes that moved go, but for those that stayed.
 */
export class TabMoves {
  private readonly moves = new Map<number, Move>();
  /** Windows that have closed, which a move may learn is its own only after (see `opened`). */
  private readonly closed = new Set<string>();

  /** Whether tab `key` is moving out. */
  has(key: number): boolean {
    return this.moves.has(key);
  }

  /**
   * Panes `panes` of tab `key` (all of them, or one) start moving to window `target` (null: a
   * new one), the sessions of those `live` with them.
   */
  start(key: number, target: string | null, panes: number[], live: number[]) {
    this.moves.set(key, { target, panes, waiting: new Set(live), stayed: new Set() });
  }

  /**
   * The new window tab `key` moves to has opened, labelled `label`: the moves over by now, if
   * it has closed already.
   */
  opened(key: number, label: string): Settled[] {
    const move = this.moves.get(key);
    if (!move) return [];
    move.target = label;
    return this.closed.has(label) ? this.windowClosed(label) : [];
  }

  /** The move of tab `key` failed before the other window had it: the tab stays as it is. */
  cancel(key: number) {
    this.moves.delete(key);
  }

  /** The moves over once the move of tab `key` has started: one without sessions is at once. */
  started(key: number): Settled[] {
    const move = this.moves.get(key);
    return move && move.waiting.size === 0 ? [this.settle(key, move)] : [];
  }

  /** Pane `pane`'s session has moved, or `stayed` here: the moves over by now. */
  resolve(pane: number, stayed: boolean): Settled[] {
    const settled: Settled[] = [];
    for (const [key, move] of this.moves) {
      if (!move.waiting.delete(pane)) continue;
      if (stayed) move.stayed.add(pane);
      if (move.waiting.size === 0) settled.push(this.settle(key, move));
    }
    return settled;
  }

  /** Window `label` closed: the moves to it are over, the sessions it hadn't taken staying here. */
  windowClosed(label: string): Settled[] {
    this.closed.add(label);
    const settled: Settled[] = [];
    for (const [key, move] of this.moves) {
      if (move.target !== label) continue;
      move.waiting.forEach((pane) => move.stayed.add(pane));
      move.waiting.clear();
      settled.push(this.settle(key, move));
    }
    return settled;
  }

  private settle(key: number, move: Move): Settled {
    this.moves.delete(key);
    return { key, gone: move.panes.filter((pane) => !move.stayed.has(pane)) };
  }
}

interface Move {
  /** The window it moves to, once known. */
  target: string | null;
  /** Its panes that move. */
  panes: number[];
  /** Its panes whose sessions haven't moved yet. */
  waiting: Set<number>;
  /** Its panes whose sessions couldn't move, which stay here. */
  stayed: Set<number>;
}

/** A move that is over: the tab, and its panes that have gone. */
export interface Settled {
  key: number;
  gone: number[];
}
