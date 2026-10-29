import {
  DEBUG_LOG_BACKEND_LINES,
  DEBUG_LOG_FRONTEND_LINES,
  type DebugLogEntry,
  type DebugLogLevel,
  type DebugLogRecord,
  type DebugLogSource,
} from "../ipc/bindings";

export type { DebugLogEntry, DebugLogLevel, DebugLogSource } from "../ipc/bindings";

/**
 * The debug log (see `debug_log/` in the backend): what the window shows of its records and
 * how, and this page's own records on their way to it. The records are English, as the
 * backend's are, and so are the lines here around them.
 */

/** The levels, from the one that matters most. */
export const LEVELS: readonly DebugLogLevel[] = ["error", "warn", "info", "debug"];

/** The records the window has of each side, oldest first. */
export interface DebugLogs {
  backend: DebugLogEntry[];
  frontend: DebugLogEntry[];
  /** Each side's `seq` up to which its records were cleared, 0 if never: those are dropped as they come. */
  clearedUpTo: Record<DebugLogSource, number>;
}

export const NO_LOGS: DebugLogs = { backend: [], frontend: [], clearedUpTo: { backend: 0, frontend: 0 } };

/** What the window shows of a side: the least a record must matter, and text its line must have (any case). */
export interface DebugLogFilter {
  level: DebugLogLevel;
  query: string;
}

const LIMITS: Record<DebugLogSource, number> = { backend: DEBUG_LOG_BACKEND_LINES, frontend: DEBUG_LOG_FRONTEND_LINES };

/**
 * `logs` with `entries` added, each to its side, in order and once (a record may come both in
 * the snapshot and in an event, in either order), keeping the latest within the backend's
 * limits and leaving out those cleared. Unchanged when nothing is new.
 */
export function addEntries(logs: DebugLogs, entries: readonly DebugLogEntry[]): DebugLogs {
  let next = logs;
  for (const source of ["backend", "frontend"] as const) {
    const list = logs[source];
    const added = entries.filter((entry) => entry.source === source && entry.seq > logs.clearedUpTo[source]);
    if (added.length === 0) continue;
    let merged: DebugLogEntry[];
    // Usually all after the last, in order.
    if (added.every((entry, i) => entry.seq > (i === 0 ? lastSeq(list) : added[i - 1].seq))) {
      merged = list.concat(added);
    } else {
      const bySeq = new Map(list.map((entry) => [entry.seq, entry]));
      for (const entry of added) bySeq.set(entry.seq, entry);
      if (bySeq.size === list.length) continue;
      merged = [...bySeq.values()].sort((a, b) => a.seq - b.seq);
    }
    next = { ...next, [source]: merged.slice(Math.max(0, merged.length - LIMITS[source])) };
  }
  return next;
}

/**
 * `logs` without `source`'s records up to `seq`, where the backend cleared them (`debug_log_clear`
 * answers with it), nor those of them still on their way here; those that came since stay.
 */
export function clearUpTo(logs: DebugLogs, source: DebugLogSource, seq: number): DebugLogs {
  return {
    ...logs,
    [source]: logs[source].filter((entry) => entry.seq > seq),
    clearedUpTo: { ...logs.clearedUpTo, [source]: Math.max(logs.clearedUpTo[source], seq) },
  };
}

/** The `seq` of the last of `list`, 0 when it is empty. */
const lastSeq = (list: readonly DebugLogEntry[]) => (list.length > 0 ? list[list.length - 1].seq : 0);

/** How many of `list` (in order) come up to record `seq`. */
function countUpTo(list: readonly DebugLogEntry[], seq: number): number {
  let [low, high] = [0, list.length];
  while (low < high) {
    const middle = (low + high) >> 1;
    if (list[middle].seq <= seq) low = middle + 1;
    else high = middle;
  }
  return low;
}

/** What the window's terminal shows: of which side, under which filter, from which of its records. */
export interface Written {
  source: DebugLogSource;
  filter: DebugLogFilter;
  /** The side's `clearedUpTo` then. */
  clearedUpTo: number;
  /** The side's records then. */
  list: readonly DebugLogEntry[];
  /** How many of `list` the filter shows. */
  shown: number;
}

/** What to write to the terminal, after what it shows, to show `logs`' records of `source` under `filter`. */
export interface Writing {
  /** The terminal is written anew, rather than added to. */
  anew: boolean;
  /** The records to write, those the filter shows. */
  entries: DebugLogEntry[];
  /** What the terminal shows then. */
  written: Written;
}

/**
 * What to write to the terminal after `before`, what it shows. New records are added at the end,
 * only those the filter shows going through it; anything else writes the terminal anew: nothing
 * written yet, another side or filter, a cleared side, or records that came before the last one
 * written (the snapshot after an event). Records the side no longer keeps (past its limit) stay
 * in the terminal, but no longer count as shown.
 */
export function nextWriting(before: Written | null, logs: DebugLogs, source: DebugLogSource, filter: DebugLogFilter): Writing {
  const list = logs[source];
  const clearedUpTo = logs.clearedUpTo[source];
  const matching = (entries: readonly DebugLogEntry[]) => entries.filter((entry) => shown(entry, filter));
  const anew = (): Writing => {
    const entries = matching(list);
    return { anew: true, entries, written: { source, filter, clearedUpTo, list, shown: entries.length } };
  };
  if (!before || before.source !== source || before.filter !== filter || before.clearedUpTo !== clearedUpTo) return anew();
  if (before.list === list) return { anew: false, entries: [], written: before };
  // Those of `list` written before: the end of `before.list`, the records before them gone.
  const kept = countUpTo(list, lastSeq(before.list));
  const dropped = before.list.length - kept;
  if (dropped < 0 || (kept > 0 && list[0].seq !== before.list[dropped].seq)) return anew();
  const entries = matching(list.slice(kept));
  const shownCount = before.shown - matching(before.list.slice(0, dropped)).length + entries.length;
  return { anew: false, entries, written: { source, filter, clearedUpTo, list, shown: shownCount } };
}

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/** The local time of `time` (milliseconds since the epoch), to the millisecond: `16:18:31.204`. */
export function clockTime(time: number): string {
  const date = new Date(time);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

/**
 * `text` as it may be written to the window's terminal: control characters, which could move
 * the cursor or redraw the screen (a server's or a gateway's words may be anything), shown as
 * escapes (`\x1b`, `\r`), and so are the line and paragraph separators. Line breaks stay, and
 * tabs become spaces.
 */
export function visible(text: string): string {
  let out = "";
  for (const char of text) {
    const code = char.codePointAt(0)!;
    if (char === "\n") out += char;
    else if (char === "\t") out += "    ";
    else if (char === "\r") out += "\\r";
    else if (code < 0x20 || (code >= 0x7f && code <= 0x9f)) out += `\\x${code.toString(16).padStart(2, "0")}`;
    else if (code === 0x2028 || code === 0x2029) out += `\\u{${code.toString(16)}}`;
    else out += char;
  }
  return out;
}

/** What a record's line shows before its message: the connection it is about, and where it comes from. */
const origin = (entry: DebugLogEntry) => (entry.connection === null ? entry.target : `#${entry.connection} ${entry.target}`);

/** A record as a line of plain text, for copying and searching. */
export function plainLine(entry: DebugLogEntry): string {
  return `${clockTime(entry.time)} ${entry.level.toUpperCase().padEnd(5)} ${origin(entry)} ${visible(entry.message)}`;
}

/** The color of each level's name (SGR), from the terminal's color scheme. */
const LEVEL_COLORS: Record<DebugLogLevel, string> = { error: "1;31", warn: "33", info: "32", debug: "2" };

/** A further line of a message, under the message's start. */
const CONTINUATION = "\r\n    ";

/**
 * A record as the window's terminal shows it, colored, with its line break. The time and the
 * origin are dimmed, the level is colored as it matters, and the message stays in the default
 * color, errors in red.
 */
export function terminalLine(entry: DebugLogEntry): string {
  const message = visible(entry.message).replace(/\n/g, CONTINUATION);
  const level = `\x1b[${LEVEL_COLORS[entry.level]}m${entry.level.toUpperCase().padEnd(5)}\x1b[0m`;
  const text = entry.level === "error" ? `\x1b[31m${message}\x1b[0m` : message;
  return `\x1b[2m${clockTime(entry.time)}\x1b[0m ${level} \x1b[2m${origin(entry)}\x1b[0m ${text}\r\n`;
}

/** Whether the window shows `entry` under `filter`. */
export function shown(entry: DebugLogEntry, { level, query }: DebugLogFilter): boolean {
  if (LEVELS.indexOf(entry.level) > LEVELS.indexOf(level)) return false;
  const words = query.trim().toLowerCase();
  return words === "" || plainLine(entry).toLowerCase().includes(words);
}

/** Something a page logged (an argument of `console.error`, an uncaught error) as text. */
export function describe(value: unknown): string {
  if (typeof value === "string") return value;
  if (value instanceof Error) {
    // WebKit's stack doesn't start with the error, as Chromium's does.
    const head = `${value.name}: ${value.message}`;
    return value.stack?.startsWith(head) ? value.stack : [head, value.stack].filter(Boolean).join("\n");
  }
  if (value !== null && typeof value === "object" && "code" in value && "message" in value) {
    // A command's error (`CommandError`).
    return `${String(value.message)} [${String(value.code)}]`;
  }
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

/** This page's records waiting to be sent: at most this many, the oldest going first. */
const PENDING_MAX = 500;
/** How long records gather before they are sent together. */
const SEND_DELAY = 200;

let pending: DebugLogRecord[] = [];
let sender: ((records: DebugLogRecord[]) => Promise<unknown>) | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Records something this page wants in the debug log, sent shortly with what comes meanwhile. */
export function report(level: DebugLogLevel, message: string) {
  if (pending.length >= PENDING_MAX) pending.shift();
  pending.push({ level, time: Date.now(), message });
  schedule();
}

/** Sends this page's records with `send` from now on (until then they wait), which may fail: they are dropped. */
export function sendRecordsWith(send: (records: DebugLogRecord[]) => Promise<unknown>) {
  sender = send;
  schedule();
}

function schedule() {
  if (!sender || timer !== undefined || pending.length === 0) return;
  timer = setTimeout(() => {
    timer = undefined;
    const records = pending;
    pending = [];
    // Not reported if it fails: that would report itself again.
    void sender?.(records).catch(() => {});
  }, SEND_DELAY);
}
