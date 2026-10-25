import type { Terminal } from "@xterm/xterm";

import { Disposables, type Disposable } from "../../lib/disposables";
import { ReadBack } from "../../lib/readBack";

/** How long the scroll bar stays after the view last scrolled, as xterm.js's own (see `.scrolling`). */
const SCROLLING_MS = 500;

/** How long the notice that output removes the lines being read back stays, fading out at the end (see `.trimming-hint`). */
const TRIMMING_SHOWN_MS = 4000;

/**
 * Reading back through the scrollback (see `lib/readBack`). The scroll bar shows while the view
 * moves (see `.scrolling`), not for output that arrives at the end or below the view.
 * `trimming` shows the notice that output removes the lines being read back (true, each time
 * it does) and hides it (false).
 */
export function followReadBack(
  term: Terminal,
  container: HTMLElement,
  trimming: (shown: boolean) => void,
): Disposable & { readBack: ReadBack } {
  const held = new Disposables();
  const readBack = held.add(new ReadBack(term));
  const scrolling = held.timer();
  const hideTrimming = held.timer();
  // Called for every line of output: most return at once.
  held.add(
    term.onScroll(() => {
      const { moved, trimmingView } = readBack.scrolled();
      if (moved) {
        container.classList.add("scrolling");
        scrolling.start(() => container.classList.remove("scrolling"), SCROLLING_MS);
      }
      if (trimmingView) {
        trimming(true);
        hideTrimming.start(() => trimming(false), TRIMMING_SHOWN_MS);
      }
    }),
  );
  held.add(term.onResize(() => readBack.resize()));
  return { readBack, dispose: () => held.dispose() };
}
