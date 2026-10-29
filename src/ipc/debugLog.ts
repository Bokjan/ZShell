import { call } from "./call";
import type { DebugLogSource } from "./bindings";
import { describe, report, sendRecordsWith } from "../lib/debugLog";

/** The debug log and its window (see `debug_log/` in the backend). */
export const debugLog = {
  /** Opens the window, or brings it forward. */
  open: () => void call("debug_log_open").catch(console.error),
  /** Both sides' records; from then on the window is sent the new ones (`debug-log-added`). */
  snapshot: () => call("debug_log_snapshot"),
  /** Forgets one side's records. */
  clear: (source: DebugLogSource) => call("debug_log_clear", { source }),
};

/**
 * Records in the debug log what goes wrong in this page: its console warnings and errors, and
 * the errors and rejected promises nothing caught (failed commands are recorded by `call`).
 * Once, as the page starts.
 */
export function recordPageErrors() {
  sendRecordsWith((records) => call("debug_log_write", { records }));
  for (const level of ["warn", "error"] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      report(level, args.map(describe).join(" "));
    };
  }
  window.addEventListener("error", (e) => report("error", `uncaught: ${describe(e.error ?? e.message)}`));
  window.addEventListener("unhandledrejection", (e) => report("error", `unhandled rejection: ${describe(e.reason)}`));
}
