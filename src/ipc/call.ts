import { invoke } from "@tauri-apps/api/core";

import type { Command, Commands } from "./bindings";

/** The arguments of command `C`: omitted when it takes none, or only optional ones. */
type Args<C extends Command> =
  Record<string, never> extends Commands[C]["args"] ? [args?: Commands[C]["args"]] : [args: Commands[C]["args"]];

/**
 * Calls backend command `command`, one of those `lib.rs` registers, with the arguments it
 * takes, resolving to what it returns (`Commands` is generated from the commands'
 * signatures). The only call of `invoke`: the other modules of `ipc/` name what they send.
 */
export const call = <C extends Command>(command: C, ...[args]: Args<C>): Promise<Commands[C]["result"]> =>
  invoke<Commands[C]["result"]>(command, args);
