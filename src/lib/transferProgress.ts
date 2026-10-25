import type { TransferProgress } from "../ipc/api";

/**
 * The progress of a file panel's transfers, by transfer id, kept apart from the transfers
 * themselves: it comes ten times a second for each one running (see `PROGRESS_INTERVAL` in
 * `sftp/transfer/`), and in the transfers' state it would render the whole panel again each
 * time, where only the transfer's own row (`TransferItem`) needs to. Kept once a transfer has
 * ended, whose row still shows how far it got, until it is forgotten.
 */
export class TransferProgresses {
  private readonly progress = new Map<string, TransferProgress>();
  private readonly listeners = new Map<string, Set<() => void>>();

  /** How far transfer `id` has got; null before it first reports. */
  get(id: string): TransferProgress | null {
    return this.progress.get(id) ?? null;
  }

  set(id: string, progress: TransferProgress) {
    this.progress.set(id, progress);
    this.listeners.get(id)?.forEach((listener) => listener());
  }

  /** Calls `listener` each time transfer `id` reports; returns the way to stop. */
  subscribe(id: string, listener: () => void): () => void {
    let set = this.listeners.get(id);
    if (!set) this.listeners.set(id, (set = new Set()));
    set.add(listener);
    return () => {
      set.delete(listener);
      if (set.size === 0 && this.listeners.get(id) === set) this.listeners.delete(id);
    };
  }

  /** Drops the progress of transfers no longer listed. */
  forget(ids: Iterable<string>) {
    for (const id of ids) this.progress.delete(id);
  }
}
