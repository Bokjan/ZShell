import { describe, expect, it } from "vitest";

import type { Notice, ProgressReport } from "./osc";
import { handleOsc, type OscSink, type OscTerminal } from "./terminalOsc";

/** A terminal that keeps the handlers registered with it, to run them as its parser would. */
class FakeTerminal implements OscTerminal {
  osc = new Map<number, (data: string) => boolean>();
  esc = new Map<string, () => boolean>();
  bufferListeners = new Set<(buffer: { type: "normal" | "alternate" }) => void>();
  screen: "normal" | "alternate" = "normal";

  parser = {
    registerOscHandler: (ident: number, handler: (data: string) => boolean) => {
      this.osc.set(ident, handler);
      return { dispose: () => this.osc.delete(ident) };
    },
    registerEscHandler: ({ final }: { final: string }, handler: () => boolean) => {
      this.esc.set(final, handler);
      return { dispose: () => this.esc.delete(final) };
    },
  };

  get buffer() {
    const term = this;
    return {
      active: {
        get type() {
          return term.screen;
        },
      },
      onBufferChange: (listener: (buffer: { type: "normal" | "alternate" }) => void) => {
        this.bufferListeners.add(listener);
        return { dispose: () => this.bufferListeners.delete(listener) };
      },
    };
  }

  /** Runs an OSC sequence; returns whether a handler took it. */
  send(ident: number, data: string): boolean | undefined {
    return this.osc.get(ident)?.(data);
  }

  switchTo(screen: "normal" | "alternate") {
    this.screen = screen;
    this.bufferListeners.forEach((listener) => listener({ type: screen }));
  }
}

function setup(copyLimit: number | null = 1024) {
  const term = new FakeTerminal();
  const seen = {
    folders: [] as string[],
    copied: [] as string[],
    progress: [] as ProgressReport[],
    notices: [] as Notice[],
    pointers: [] as string[],
  };
  const sink: OscSink = {
    folder: (path) => seen.folders.push(path),
    copyLimit: () => copyLimit,
    copy: (text) => seen.copied.push(text),
    progress: (report) => seen.progress.push(report),
    notify: (notice) => seen.notices.push(notice),
    pointer: (shape) => seen.pointers.push(shape),
  };
  const handlers = handleOsc(term, sink);
  return { term, seen, handlers };
}

describe("handleOsc", () => {
  it("reports the shell's folder, and takes what it can't read too", () => {
    const { term, seen } = setup();
    expect(term.send(7, "file://host/home/me%20too")).toBe(true);
    expect(term.send(7, "nonsense")).toBe(true);
    expect(seen.folders).toEqual(["/home/me too"]);
  });

  it("copies what a program copies only while the settings let it, up to their limit", () => {
    const { term, seen } = setup(4);
    term.send(52, `c;${btoa("text")}`);
    term.send(52, `c;${btoa("longer")}`);
    term.send(52, "c;?");
    expect(seen.copied).toEqual(["text"]);
    const off = setup(null);
    expect(off.term.send(52, `c;${btoa("text")}`)).toBe(true);
    expect(off.seen.copied).toEqual([]);
  });

  it("passes on progress and notifications", () => {
    const { term, seen } = setup();
    term.send(9, "4;1;50");
    term.send(9, "Built");
    term.send(777, "notify;Done;All of it");
    term.send(99, "i=1:d=0;Half");
    expect(seen.notices).toHaveLength(2);
    term.send(99, "i=1;way");
    expect(seen.progress).toEqual([{ state: "normal", percent: 50 }]);
    expect(seen.notices.map(({ title, body }) => [title, body])).toEqual([
      ["", "Built"],
      ["Done", "All of it"],
      ["Halfway", ""],
    ]);
  });

  it("keeps a pointer shape for each screen, the alternate screen's going when it is left", () => {
    const { term, seen } = setup();
    term.send(22, "pointer");
    term.switchTo("alternate");
    term.send(22, "text");
    term.switchTo("normal");
    term.switchTo("alternate");
    expect(seen.pointers).toEqual(["pointer", "", "text", "pointer", ""]);
  });

  it("puts back the terminal's own pointer on a full reset, leaving the reset to the terminal", () => {
    const { term, seen, handlers } = setup();
    term.send(22, "pointer");
    expect(term.esc.get("c")?.()).toBe(false);
    term.send(22, "wait");
    handlers.resetPointer();
    expect(seen.pointers).toEqual(["pointer", "", "wait", ""]);
  });

  it("handles nothing once disposed", () => {
    const { term, handlers } = setup();
    handlers.dispose();
    expect(term.osc.size).toBe(0);
    expect(term.esc.size).toBe(0);
    expect(term.bufferListeners.size).toBe(0);
  });
});
