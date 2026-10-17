import { errorMessage, updates } from "../ipc/api";
import type { Release, UpdateStatus } from "../ipc/bindings";
import { listenHere } from "../ipc/events";
import { createStore } from "./store";

export interface UpdateState {
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

/** The state of the update check in this window. */
export const updateState = createStore<UpdateState>({ loaded: false, supported: false, newer: null, checking: false, result: null });

const apply = (status: UpdateStatus) => updateState.set({ loaded: true, supported: status.supported, newer: status.newer });

let started = false;

/** Reads the status once per page, and follows the checks of any window (see update.rs). */
export function followUpdates() {
  if (started) return;
  started = true;
  updates.status().then(apply, console.error);
  // A check that worked (the automatic one, or another window's) clears the error of one that didn't.
  void listenHere("update-status", (e) => {
    apply(e.payload);
    if (updateState.get().result?.kind === "failed") updateState.set({ result: null });
  });
}

/** Checks now, as the user asked; the result is shown in Settings › About. */
export function checkForUpdates() {
  const { checking, supported } = updateState.get();
  if (checking || !supported) return;
  updateState.set({ checking: true, result: null });
  updates.check().then(
    (status) => {
      apply(status);
      updateState.set({ checking: false, result: status.newer ? null : { kind: "upToDate" } });
    },
    (e) => updateState.set({ checking: false, result: { kind: "failed", message: errorMessage(e) } }),
  );
}
