/**
 * What programs ask of the terminal with OSC sequences beyond those xterm.js handles itself
 * (titles, colors, links) and OSC 7 (`shellDirs`): copying to the clipboard (52), progress
 * (9;4), notifications (9, 777 and 99) and the pointer's shape (22). Each takes the sequence's
 * data, after its number and `;`.
 */

import type { TaskState } from "../ipc/bindings";

/**
 * The text a program copies (OSC 52 `Pc;Pd`, `Pd` in base64): null for a query (`?`, never
 * answered: programs don't read the clipboard), for clearing it (empty), for text past `limit`
 * bytes and anything malformed. Every selection (`c`, `p`, `s`, none) is the clipboard.
 */
export function osc52Text(data: string, limit: number): string | null {
  const split = data.indexOf(";");
  if (split < 0 || !/^[cpqs0-7]*$/.test(data.slice(0, split))) return null;
  const encoded = data.slice(split + 1);
  if (encoded === "" || encoded === "?" || (encoded.length / 4) * 3 > limit + 2) return null;
  try {
    const bytes = Uint8Array.from(atob(encoded), (c) => c.charCodeAt(0));
    if (bytes.length > limit) return null;
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/** A progress report: a state with a percentage (none: the one before stays), or none any more. */
export type ProgressReport = { state: TaskState; percent: number | null } | null;

const PROGRESS_STATES: (TaskState | null)[] = [null, "normal", "error", "indeterminate", "paused"];

/**
 * OSC 9: ConEmu's commands, which begin with a number (only progress, `4;state;percent`, is
 * used), or else iTerm2's notification with its text. Null for anything else.
 */
export function parseOsc9(data: string): { progress: ProgressReport } | { notify: string } | null {
  if (!/^\d+(;|$)/.test(data)) return data.trim() ? { notify: data } : null;
  const [command, state = "0", percent] = data.split(";");
  if (command !== "4" || !/^\d$/.test(state) || Number(state) >= PROGRESS_STATES.length) return null;
  const kind = PROGRESS_STATES[Number(state)];
  if (!kind) return { progress: null };
  const value = percent !== undefined && /^\d+$/.test(percent) ? Math.min(Number(percent), 100) : null;
  return { progress: { state: kind, percent: value } };
}

/** A notification: a title (perhaps empty) and its text (perhaps empty, not both). */
export interface Notice {
  title: string;
  body: string;
}

/** OSC 777 (urxvt, Ghostty): `notify;title;body`; null for its other commands. */
export function parseOsc777(data: string): Notice | null {
  const [command, title = "", ...body] = data.split(";");
  if (command !== "notify") return null;
  const notice = { title: title.trim(), body: body.join(";").trim() };
  return notice.title || notice.body ? notice : null;
}

/** The most text a kitty notification gathers, in characters, and how many can be unfinished at once. */
const KITTY_TEXT_MAX = 4096;
const KITTY_PENDING_MAX = 8;

/**
 * kitty's notifications (OSC 99 `metadata;payload`), which may come in parts with the same
 * `i` until one without `d=0`: their title and body, in plain text or base64 (`e=1`). The
 * rest of the protocol (buttons, icons, closing, queries) is not supported, and not answered.
 */
export class KittyNotices {
  private pending = new Map<string, Notice>();

  /** Takes a sequence; returns the notification it completes, if any. */
  feed(data: string): Notice | null {
    const split = data.indexOf(";");
    if (split < 0) return null;
    const meta = new Map<string, string>();
    for (const item of data.slice(0, split).split(":")) {
      const at = item.indexOf("=");
      if (at > 0) meta.set(item.slice(0, at), item.slice(at + 1));
    }
    const id = meta.get("i") ?? "";
    const part = meta.get("p") ?? "title";
    if (part !== "title" && part !== "body") {
      if (part === "close") this.pending.delete(id);
      return null;
    }
    const text = meta.get("e") === "1" ? decodeBase64(data.slice(split + 1)) : data.slice(split + 1);
    if (text === null) return null;
    const notice = this.pending.get(id) ?? { title: "", body: "" };
    this.pending.delete(id);
    notice[part] = (notice[part] + text).slice(0, KITTY_TEXT_MAX);
    if (meta.get("d") === "0") {
      this.pending.set(id, notice);
      // The oldest unfinished one goes first: a program that never finishes can't fill memory.
      if (this.pending.size > KITTY_PENDING_MAX) this.pending.delete(this.pending.keys().next().value!);
      return null;
    }
    const done = { title: notice.title.trim(), body: notice.body.trim() };
    return done.title || done.body ? done : null;
  }
}

function decodeBase64(text: string): string | null {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(atob(text), (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}

/** The pointer shapes of CSS, which OSC 22 names. */
const CSS_POINTERS = new Set([
  "default",
  "none",
  "context-menu",
  "help",
  "pointer",
  "progress",
  "wait",
  "cell",
  "crosshair",
  "text",
  "vertical-text",
  "alias",
  "copy",
  "move",
  "no-drop",
  "not-allowed",
  "grab",
  "grabbing",
  "all-scroll",
  "col-resize",
  "row-resize",
  "n-resize",
  "e-resize",
  "s-resize",
  "w-resize",
  "ne-resize",
  "nw-resize",
  "se-resize",
  "sw-resize",
  "ew-resize",
  "ns-resize",
  "nesw-resize",
  "nwse-resize",
  "zoom-in",
  "zoom-out",
]);

/** The X11 cursor font's names that xterm and kitty also take, as CSS names. */
const X11_POINTERS: Record<string, string> = {
  left_ptr: "default",
  arrow: "default",
  xterm: "text",
  hand1: "pointer",
  hand2: "pointer",
  question_arrow: "help",
  watch: "wait",
  left_ptr_watch: "progress",
  crosshair: "crosshair",
  tcross: "crosshair",
  cross: "crosshair",
  plus: "cell",
  fleur: "move",
  sb_h_double_arrow: "ew-resize",
  sb_v_double_arrow: "ns-resize",
  top_side: "n-resize",
  bottom_side: "s-resize",
  left_side: "w-resize",
  right_side: "e-resize",
  top_left_corner: "nw-resize",
  top_right_corner: "ne-resize",
  bottom_left_corner: "sw-resize",
  bottom_right_corner: "se-resize",
};

/** A name of OSC 22 as CSS: null for one we don't know. */
const cssPointer = (name: string) => (CSS_POINTERS.has(name) ? name : (X11_POINTERS[name] ?? null));

/** The most shapes a screen's stack keeps. */
const POINTER_STACK_MAX = 16;

/**
 * The pointer shape a program sets over its terminal (OSC 22), as kitty keeps it: a stack for
 * each screen, so that the shell's comes back when a full-screen program leaves. A name (or
 * `=name`) replaces the top shape, `>name` pushes one, `<` pops one; of a list of names
 * (`a,b`) the first known one is used. Queries (`?`) are not answered.
 */
export class PointerShapes {
  private stacks = { normal: [] as string[], alternate: [] as string[] };

  /** Takes a sequence on `screen`; returns whether that screen's shape changed. */
  feed(data: string, screen: "normal" | "alternate"): boolean {
    const stack = this.stacks[screen];
    const before = this.shape(screen);
    const op = /^[=<>]/.test(data) ? data[0] : "=";
    const names = (op === "=" && data[0] !== "=" ? data : data.slice(1)).split(",").map((n) => n.trim());
    if (op === "<") stack.pop();
    else if (names.some((n) => n.startsWith("?"))) return false;
    else {
      // Empty: the default shape.
      const shape = names.length === 1 && names[0] === "" ? "" : names.map(cssPointer).find((css) => css !== null);
      if (shape === undefined) return false;
      if (op === "=") stack.pop();
      stack.push(shape);
      if (stack.length > POINTER_STACK_MAX) stack.shift();
    }
    return this.shape(screen) !== before;
  }

  /** The CSS shape on `screen`; empty for the terminal's own. */
  shape(screen: "normal" | "alternate"): string {
    const stack = this.stacks[screen];
    return stack[stack.length - 1] ?? "";
  }

  /** The terminal is reset: every shape goes. */
  reset() {
    this.stacks = { normal: [], alternate: [] };
  }
}
