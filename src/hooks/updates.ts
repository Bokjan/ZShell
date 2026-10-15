import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

import { errorMessage } from "../ipc/api";
import type { Release, UpdateStatus } from "../ipc/bindings";
import { useSettings } from "./settings";
import { listenHere } from "../ipc/events";

export type { Release } from "../ipc/bindings";

interface State {
  /** Whether the status has been read from the backend. */
  loaded: boolean;
  /** False in the Microsoft Store package, which never checks. */
  supported: boolean;
  /** The latest release, when it is newer than this version. */
  newer: Release | null;
  checking: boolean;
  /** What the check the user asked for found: nothing newer, or an error. */
  result: { kind: "upToDate" } | { kind: "failed"; message: string } | null;
}

let state: State = { loaded: false, supported: false, newer: null, checking: false, result: null };
const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}

const apply = (status: UpdateStatus) => set({ loaded: true, supported: status.supported, newer: status.newer });

let started = false;

/** Reads the status once per page, and follows the checks of any window (see update.rs). */
function start() {
  if (started) return;
  started = true;
  invoke<UpdateStatus>("update_status").then(apply, console.error);
  // A check that worked (the automatic one, or another window's) clears the error of one that didn't.
  void listenHere<UpdateStatus>("update-status", (e) => {
    apply(e.payload);
    if (state.result?.kind === "failed") set({ result: null });
  });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

/** Checks now, as the user asked; the result is shown in Settings › About. */
export function checkForUpdates() {
  if (state.checking || !state.supported) return;
  set({ checking: true, result: null });
  invoke<UpdateStatus>("update_check").then(
    (status) => {
      apply(status);
      set({ checking: false, result: status.newer ? null : { kind: "upToDate" } });
    },
    (e) => set({ checking: false, result: { kind: "failed", message: errorMessage(e) } }),
  );
}

/**
 * The state of the update check, and `available`: the newer release unless the user skipped
 * it, which the app marks (the menu button, Settings › About).
 */
export function useUpdates() {
  useEffect(start, []);
  const current = useSyncExternalStore(subscribe, () => state);
  const { settings } = useSettings();
  const available = current.newer && current.newer.version !== settings.updates.skipped ? current.newer : null;
  return { ...current, available };
}
