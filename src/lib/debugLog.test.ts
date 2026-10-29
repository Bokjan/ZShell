import { afterEach, describe as group, expect, it, vi } from "vitest";

import { DEBUG_LOG_FRONTEND_LINES } from "../ipc/bindings";
import {
  addEntries,
  clearUpTo,
  clockTime,
  describe,
  nextWriting,
  NO_LOGS,
  plainLine,
  report,
  sendRecordsWith,
  shown,
  terminalLine,
  visible,
  type DebugLogEntry,
  type DebugLogFilter,
  type DebugLogs,
} from "./debugLog";

const at = new Date(2026, 9, 29, 16, 18, 31, 204).getTime();

const entry = (seq: number, fields: Partial<DebugLogEntry> = {}): DebugLogEntry => ({
  seq,
  source: "backend",
  time: at,
  level: "debug",
  target: "ssh",
  connection: null,
  message: `record ${seq}`,
  ...fields,
});

const seqs = (logs: DebugLogs) => ({ backend: logs.backend.map((e) => e.seq), frontend: logs.frontend.map((e) => e.seq) });

group("addEntries", () => {
  const empty = NO_LOGS;

  it("adds each record to its side, in order and once", () => {
    const logs = addEntries(empty, [entry(1), entry(2, { source: "frontend" }), entry(3)]);
    expect(seqs(logs)).toEqual({ backend: [1, 3], frontend: [2] });
    // An event that came before the snapshot, then the snapshot with the same record.
    const early = addEntries(empty, [entry(5), entry(6)]);
    expect(seqs(addEntries(early, [entry(4), entry(5)]))).toEqual({ backend: [4, 5, 6], frontend: [] });
    expect(addEntries(logs, [entry(1)])).toBe(logs);
    expect(addEntries(logs, [])).toBe(logs);
  });

  it("keeps the latest within the side's limit", () => {
    const many = Array.from({ length: DEBUG_LOG_FRONTEND_LINES + 5 }, (_, i) => entry(i + 1, { source: "frontend" }));
    const logs = addEntries(empty, many);
    expect(logs.frontend).toHaveLength(DEBUG_LOG_FRONTEND_LINES);
    expect(logs.frontend[0].seq).toBe(6);
  });
});

it("drops what was cleared, even when it comes after", () => {
  const logs = addEntries(NO_LOGS, [entry(1), entry(2, { source: "frontend" }), entry(3), entry(4)]);
  // Record 4 came after the backend cleared up to 3, before its answer.
  const cleared = clearUpTo(logs, "backend", 3);
  expect(seqs(cleared)).toEqual({ backend: [4], frontend: [2] });
  // A batch sent before the clearing, heard after.
  expect(seqs(addEntries(cleared, [entry(2), entry(3), entry(5)]))).toEqual({ backend: [4, 5], frontend: [2] });
  expect(clearUpTo(cleared, "backend", 1).clearedUpTo.backend).toBe(3);
});

group("nextWriting", () => {
  const filter = { level: "debug" as const, query: "" };
  const warnings = { level: "warn" as const, query: "" };
  const logsOf = (...entries: DebugLogEntry[]): DebugLogs => ({ ...NO_LOGS, backend: entries });
  const ids = (writing: { entries: DebugLogEntry[] }) => writing.entries.map((e) => e.seq);
  const at = (logs: DebugLogs, f: DebugLogFilter = filter) => nextWriting(null, logs, "backend", f).written;

  it("adds the records after those written, counting those shown", () => {
    const first = nextWriting(null, logsOf(entry(1), entry(2)), "backend", filter);
    expect([first.anew, ids(first), first.written.shown]).toEqual([true, [1, 2], 2]);
    const next = nextWriting(first.written, logsOf(entry(1), entry(2), entry(3)), "backend", filter);
    expect([next.anew, ids(next), next.written.shown]).toEqual([false, [3], 3]);
    // Nothing new for this side.
    const same = nextWriting(next.written, { ...NO_LOGS, backend: next.written.list as DebugLogEntry[] }, "backend", filter);
    expect([same.anew, ids(same)]).toEqual([false, []]);
    // The oldest dropped at the side's limit stop counting.
    const shifted = nextWriting(
      at(logsOf(entry(1), entry(2), entry(3)), warnings),
      logsOf(entry(2), entry(3), entry(4, { level: "warn" })),
      "backend",
      warnings,
    );
    expect([shifted.anew, ids(shifted), shifted.written.shown]).toEqual([false, [4], 1]);
    const all = logsOf(entry(1, { level: "warn" }), entry(2, { level: "warn" }));
    const replaced = nextWriting(at(all, warnings), logsOf(entry(5), entry(6, { level: "error" })), "backend", warnings);
    expect([replaced.anew, ids(replaced), replaced.written.shown]).toEqual([false, [6], 1]);
  });

  it("writes anew for another side or filter, a cleared side, or records before the last written", () => {
    const logs = logsOf(entry(1), entry(2), entry(3));
    const written = at(logsOf(entry(2), entry(3)));
    expect(nextWriting(written, logs, "frontend", filter).anew).toBe(true);
    expect(nextWriting(written, logs, "backend", { ...filter }).anew).toBe(true);
    // The snapshot's record 1 came after the event's 2 and 3.
    expect(nextWriting(written, logs, "backend", filter).anew).toBe(true);
    // Cleared, with records written meanwhile kept.
    const cleared = nextWriting(at(logs), clearUpTo(logsOf(entry(1), entry(2), entry(3), entry(4)), "backend", 2), "backend", filter);
    expect([cleared.anew, ids(cleared), cleared.written.shown]).toEqual([true, [3, 4], 2]);
  });
});

it("shows the local time to the millisecond", () => {
  expect(clockTime(at)).toBe("16:18:31.204");
  expect(clockTime(new Date(2026, 0, 1, 1, 2, 3, 4).getTime())).toBe("01:02:03.004");
});

it("makes control characters visible, keeping line breaks", () => {
  expect(visible("HTTP/1.1 403\r\n\x1b[2Jno\tbell\x07 \x9b x ")).toBe("HTTP/1.1 403\\r\n\\x1b[2Jno    bell\\x07 \\x9b x\\u{2028}");
  expect(visible("plain 中文")).toBe("plain 中文");
});

it("writes a record as a line", () => {
  const record = entry(1, { level: "warn", connection: 12, message: "the server sent\x1b[2J" });
  expect(plainLine(record)).toBe("16:18:31.204 WARN  #12 ssh the server sent\\x1b[2J");
  const line = terminalLine(entry(2, { level: "error", message: "first\nsecond" }));
  expect(line).toBe("\x1b[2m16:18:31.204\x1b[0m \x1b[1;31mERROR\x1b[0m \x1b[2mssh\x1b[0m \x1b[31mfirst\r\n    second\x1b[0m\r\n");
  // Nothing but the colors and its own line breaks act on the terminal.
  expect(terminalLine(entry(3, { message: "\x1b]0;title\x07\r" })).replace(/\x1b\[[0-9;]*m/g, "")).not.toMatch(
    /[\x00-\x09\x0b\x0c\x0e-\x1f]/,
  );
});

it("filters by level and by text in any case", () => {
  const record = entry(1, { level: "info", connection: 12, message: "Authenticated as root" });
  expect(shown(record, { level: "debug", query: "" })).toBe(true);
  expect(shown(record, { level: "info", query: "" })).toBe(true);
  expect(shown(record, { level: "warn", query: "" })).toBe(false);
  expect(shown(record, { level: "debug", query: " AUTHENTICATED " })).toBe(true);
  expect(shown(record, { level: "debug", query: "#12" })).toBe(true);
  expect(shown(record, { level: "debug", query: "#13" })).toBe(false);
  expect(shown(record, { level: "debug", query: "16:18" })).toBe(true);
});

it("describes what pages log", () => {
  expect(describe("text")).toBe("text");
  expect(describe({ code: "vault.wrongPassword", message: "Wrong password" })).toBe("Wrong password [vault.wrongPassword]");
  expect(describe({ a: 1 })).toBe('{"a":1}');
  expect(describe(undefined)).toBe("undefined");
  const error = new TypeError("bad");
  expect(describe(error).startsWith("TypeError: bad")).toBe(true);
  const circular: Record<string, unknown> = {};
  circular.self = circular;
  expect(describe(circular)).toBe("[object Object]");
});

group("report", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("sends records together, once there is somewhere to send them", async () => {
    vi.useFakeTimers();
    report("warn", "before");
    const sent: string[][] = [];
    sendRecordsWith(async (records) => sent.push(records.map((record) => record.message)));
    report("error", "after");
    expect(sent).toEqual([]);
    await vi.runAllTimersAsync();
    expect(sent).toEqual([["before", "after"]]);
    report("info", "later");
    await vi.runAllTimersAsync();
    expect(sent).toEqual([["before", "after"], ["later"]]);
  });
});
