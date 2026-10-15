import { invoke, type InvokeArgs } from "@tauri-apps/api/core";

import type { Command } from "./bindings";

/**
 * Calls backend command `command`, one of those `lib.rs` registers (`Command` is generated
 * from them). The only call of `invoke`: the other modules of `ipc/` name what they send.
 */
export const call = <T = void>(command: Command, args?: InvokeArgs) => invoke<T>(command, args);
