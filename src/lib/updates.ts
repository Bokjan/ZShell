import { errorMessage, updates } from "../ipc/api";
import type { Release, UpdateStatus } from "../ipc/bindings";
import { listenHere } from "../ipc/events";
import { latestOnly } from "./latestOnly";
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

// A status the backend sends or a check answers is newer than one read before it (see `latestOnly`).
const statuses = latestOnly((status: UpdateStatus) => updateState.set({ loaded: true, supported: status.supported, newer: status.newer }));

let started = false;

/**
 * Reads the status once per page, and follows the checks of any window (see update.rs):
 * listening first, so that none is missed while the status is read.
 */
export function followUpdates() {
  if (started) return;
  started = true;
  // A check that worked (the automatic one, or another window's) clears the error of one that didn't.
  const listening = listenHere("update-status", (e) => {
    statuses.changed(e.payload);
    if (updateState.get().result?.kind === "failed") updateState.set({ result: null });
  }).catch(console.error);
  void listening.then(() => statuses.track(updates.status())).catch(console.error);
}

/** Checks now, as the user asked; the result is shown in About. */
export function checkForUpdates() {
  const { checking, supported } = updateState.get();
  if (checking || !supported) return;
  updateState.set({ checking: true, result: null });
  updates.check().then(
    (status) => {
      statuses.changed(status);
      updateState.set({ checking: false, result: status.newer ? null : { kind: "upToDate" } });
    },
    (e) => updateState.set({ checking: false, result: { kind: "failed", message: errorMessage(e) } }),
  );
}
