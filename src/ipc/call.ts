import { invoke } from "@tauri-apps/api/core";

import type { Command, Commands } from "./bindings";
import { describe, report } from "../lib/debugLog";

/** The arguments of command `C`: omitted when it takes none, or only optional ones. */
type Args<C extends Command> =
  Record<string, never> extends Commands[C]["args"] ? [args?: Commands[C]["args"]] : [args: Commands[C]["args"]];

/**
 * Calls backend command `command`, one of those `lib.rs` registers, with the arguments it
 * takes, resolving to what it returns (`Commands` is generated from the commands'
 * signatures). The only call of `invoke`: the other modules of `ipc/` name what they send.
 *
 * A command that fails is recorded in the debug log, by its name and error alone: the
 * arguments may hold passwords. Not the debug log's own, which would record itself.
 */
export const call = <C extends Command>(command: C, ...[args]: Args<C>): Promise<Commands[C]["result"]> => {
  const result = invoke<Commands[C]["result"]>(command, args);
  if (command !== "debug_log_write") result.catch((e: unknown) => report("info", `command ${command} failed: ${describe(e)}`));
  return result;
};
