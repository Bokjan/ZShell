import type { Terminal } from "@xterm/xterm";
import type { SerializeAddon } from "@xterm/addon-serialize";

import type { TerminalPort } from "../../lib/paneSession";
import type { TerminalScreen } from "../../lib/sessionRegistry";
import { extraModes } from "../../lib/terminalModes";

/** How the pane's session reads and writes the terminal. */
export function terminalPort(term: Terminal, serialize: SerializeAddon, fit: { fit(): void }): TerminalPort {
  return {
    write: (data, done) => term.write(data, done),
    size: () => ({ cols: term.cols, rows: term.rows }),
    lineBeforeCursor: () => {
      const buffer = term.buffer.active;
      return buffer.getLine(buffer.baseY + buffer.cursorY)?.translateToString(false, 0, buffer.cursorX) ?? "";
    },
    alternateScreen: () => term.buffer.active.type === "alternate",
    // With the modes programs set (mouse reporting, bracketed paste and so on), and what
    // the addon doesn't keep.
    snapshot: () => ({ data: serialize.serialize() + extraModes(term), cols: term.cols, rows: term.rows }),
    // Drawn at the size it was taken at, as the addon advises; fitted to the pane after.
    restore: (snapshot, done) => {
      term.resize(snapshot.cols, snapshot.rows);
      term.write(snapshot.data, done);
    },
    fit: () => fit.fit(),
  };
}

/** The terminal as the status bar shows it. */
export function terminalScreen(term: Terminal): TerminalScreen {
  return {
    size: () => ({ cols: term.cols, rows: term.rows }),
    cursor: () => {
      const buffer = term.buffer.active;
      // Past the last column while a line waits to wrap.
      return { row: buffer.cursorY + 1, column: Math.min(buffer.cursorX, term.cols - 1) + 1 };
    },
    subscribe: (listener) => {
      const events = [term.onCursorMove(listener), term.onResize(listener), term.buffer.onBufferChange(listener)];
      return () => events.forEach((e) => e.dispose());
    },
  };
}
