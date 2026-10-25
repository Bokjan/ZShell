import { Disposables, type Disposable } from "./disposables";
import { KittyNotices, PointerShapes, osc52Text, parseOsc777, parseOsc9, type Notice, type ProgressReport } from "./osc";
import { parseOsc7 } from "./shellDirs";

type Screen = "normal" | "alternate";

/** What of a terminal the OSC handlers use, as xterm.js's `Terminal`. */
export interface OscTerminal {
  parser: {
    /** The handler returns whether it took the sequence. */
    registerOscHandler(ident: number, handler: (data: string) => boolean): Disposable;
    registerEscHandler(id: { final: string }, handler: () => boolean): Disposable;
  };
  buffer: {
    active: { type: Screen };
    onBufferChange(listener: (buffer: { type: Screen }) => void): Disposable;
  };
}

/** Where what programs ask for goes, for the pane whose terminal it is. */
export interface OscSink {
  /** The folder the shell reports (OSC 7). */
  folder(path: string): void;
  /** The most bytes a program may copy (OSC 52), or null while the settings don't let it. */
  copyLimit(): number | null;
  copy(text: string): void;
  /** Progress (ConEmu's OSC 9;4). */
  progress(report: ProgressReport): void;
  /** Notifications (iTerm2's OSC 9, OSC 777, kitty's OSC 99). */
  notify(notice: Notice): void;
  /** The pointer shape over the terminal (OSC 22), as CSS; empty for the terminal's own. */
  pointer(shape: string): void;
}

/** The handlers of a terminal; `resetPointer` puts back its own pointer shape, over any a program set. */
export interface OscHandlers extends Disposable {
  resetPointer(): void;
}

/**
 * Handles what programs ask with OSC sequences beyond what xterm.js does itself (see `osc`),
 * for as long as the result isn't disposed. Asking for the clipboard's text is never answered.
 */
export function handleOsc(term: OscTerminal, sink: OscSink): OscHandlers {
  const held = new Disposables();
  const osc = (ident: number, handler: (data: string) => void) =>
    held.add(
      term.parser.registerOscHandler(ident, (data) => {
        handler(data);
        return true;
      }),
    );
  osc(7, (data) => {
    const path = parseOsc7(data);
    if (path) sink.folder(path);
  });
  osc(52, (data) => {
    const limit = sink.copyLimit();
    const text = limit === null ? null : osc52Text(data, limit);
    if (text !== null) sink.copy(text);
  });
  osc(9, (data) => {
    const parsed = parseOsc9(data);
    if (parsed && "progress" in parsed) sink.progress(parsed.progress);
    else if (parsed) sink.notify(parsed.notice);
  });
  osc(777, (data) => {
    const notice = parseOsc777(data);
    if (notice) sink.notify(notice);
  });
  const kittyNotices = new KittyNotices();
  osc(99, (data) => {
    const notice = kittyNotices.feed(data);
    if (notice) sink.notify(notice);
  });
  // The pointer shape programs set, for each screen.
  const pointers = new PointerShapes();
  const showPointer = () => sink.pointer(pointers.shape(term.buffer.active.type));
  const resetPointer = () => {
    pointers.reset();
    showPointer();
  };
  osc(22, (data) => {
    if (pointers.feed(data, term.buffer.active.type)) showPointer();
  });
  held.add(
    // The alternate screen's shapes go with its text, so that the next full-screen program
    // doesn't start with those the last one left.
    term.buffer.onBufferChange((buffer) => {
      if (buffer.type === "normal") pointers.clear("alternate");
      showPointer();
    }),
  );
  held.add(
    // A full reset (RIS) puts back the terminal's own pointer too; xterm.js resets the rest.
    term.parser.registerEscHandler({ final: "c" }, () => {
      resetPointer();
      return false;
    }),
  );
  return { resetPointer, dispose: () => held.dispose() };
}
