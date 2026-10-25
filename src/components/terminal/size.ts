import type { Terminal } from "@xterm/xterm";

import { Disposables, type Disposable } from "../../lib/disposables";
import type { PatchStore } from "../../lib/store";
import { COLS_DEBOUNCE_MS, resizeStep, type FullWidthFitAddon } from "../../lib/terminalFit";
import type { SizeShown } from "./SizeOverlay";

/**
 * How long the terminal's size shows after it changes: a second, then it fades out (see
 * `.size-overlay`). Not in a pane that opened less than `SIZE_SETTLE_MS` ago, whose terminal is
 * still settling into its place.
 */
const SIZE_SHOWN_MS = 1150;
const SIZE_SETTLE_MS = 500;

export interface FitDeps {
  /** Whether a screen from another window is being drawn, at that window's size. */
  arriving(): boolean;
  visible(): boolean;
  /** Resized while hidden: to fit once shown. */
  unfitted(): void;
}

/** Fits the terminal to its pane (`fit`), except while a screen from another window is drawn. */
export interface Fitter extends Disposable {
  fit(): void;
}

/**
 * Fits the terminal to `container` as it changes size. A hidden tab keeps its size, and resizing
 * the window would refit (and reflow) every terminal in every tab on each frame: those out of
 * sight fit once shown again, except the first time (a tab opened in the background starts its
 * session at its size).
 */
export function followSize(term: Terminal, addon: FullWidthFitAddon, container: HTMLElement, deps: FitDeps): Fitter {
  const held = new Disposables();
  const colsLater = held.timer();
  const fit = () => {
    if (!deps.arriving()) addon.fit();
  };
  // As the window's edge or a splitter is dragged (see `resizeStep`).
  const follow = () => {
    const proposed = addon.proposeDimensions();
    if (deps.arriving() || !proposed) return;
    const step = resizeStep({ cols: term.cols, rows: term.rows }, proposed, term.buffer.active.length);
    if (step.now) addon.resizeTo(step.now);
    colsLater.clear();
    if (step.colsLater) {
      colsLater.start(() => {
        if (deps.visible()) fit();
        else deps.unfitted();
      }, COLS_DEBOUNCE_MS);
    }
  };
  let fitted = false;
  const observer = new ResizeObserver(() => {
    if (!fitted) {
      fitted = true;
      fit();
    } else if (deps.visible()) {
      follow();
    } else {
      deps.unfitted();
    }
  });
  observer.observe(container);
  held.defer(() => observer.disconnect());
  return { fit, dispose: () => held.dispose() };
}

/** Shows the terminal's size in `shown` as it changes, while it is in sight (`visible`). */
export function showSize(term: Terminal, shown: PatchStore<SizeShown>, visible: () => boolean): Disposable {
  const held = new Disposables();
  const hide = held.timer();
  const opened = Date.now();
  held.add(
    term.onResize(({ cols, rows }) => {
      // Out of sight (a hidden tab resized with the window), nobody would see it.
      if (!visible() || Date.now() - opened < SIZE_SETTLE_MS) return;
      shown.set({ size: { cols, rows, count: (shown.get().size?.count ?? 0) + 1 } });
      hide.start(() => shown.set({ size: null }), SIZE_SHOWN_MS);
    }),
  );
  return held;
}
