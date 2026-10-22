/**
 * A value saved whole to the backend (the settings, the quick commands), changed here as a
 * change to it, shown at once. Until it is stored, a change is kept as a change: when another
 * window stores the value meanwhile (`reload`), the changes made here are made again to what
 * it stored, so that neither window's change is lost, as they would be if either stored the
 * whole value it showed. Only the latest request's answer is shown: an older one would undo a
 * newer change, here or in another window.
 *
 * A save that fails puts back the stored value, which the change never reached, and is
 * reported: otherwise the change would stay on screen until the next start. A failure that
 * newer changes have overtaken is neither: their save carries the failed change too.
 *
 * A save can wait (a field saved as it is typed, which would otherwise write the file on each
 * key): the change is shown at once and stored once no other change has come for `delay` ms,
 * or when flushed.
 *
 * One request is with the backend at a time, which keeps them in order (its commands run
 * apart, off the main thread, as each writes a file): the others wait, a newer store taking
 * the place of one waiting to be made, so that changes made faster than the disk takes them
 * (a number stepped with a key held down) are stored as they can be, the last one always.
 */
export interface LatestSave<T> {
  /** Shows `change` made to the value shown and saves it, after `delay` ms without another change if given. */
  save(change: (value: T) => T, delay?: number): void;
  /** Stores a change still waiting, if any. */
  flush(): void;
  /** Shows the stored value (after a change in another window), with the changes not stored yet. */
  reload(): void;
  /**
   * The stored value, read elsewhere (when the page starts). Ignored once a newer one has
   * come: a reload (another window changed it while this read was on its way) or a store.
   */
  loaded(value: T): void;
}

export function latestSave<T>(options: {
  show(value: T): void;
  store(value: T): Promise<T>;
  load(): Promise<T>;
  failed(error: unknown): void;
}): LatestSave<T> {
  // The value as last stored or loaded, and the changes made since that aren't stored yet,
  // oldest first: what is shown is those changes made to it.
  let base: T | undefined;
  let shown: T | undefined;
  const pending: ((value: T) => T)[] = [];
  let waiting: ReturnType<typeof setTimeout> | null = null;
  let latest = 0;
  // The requests after the one with the backend, in order: stores and loads.
  const queue: { kind: "store" | "load"; run(): Promise<void> }[] = [];
  let busy = false;
  const next = () => {
    const request = queue.shift();
    busy = request !== undefined;
    request?.run().then(next, next);
  };
  const enqueue = (kind: "store" | "load", run: (request: number) => Promise<void>) => {
    const request = ++latest;
    // A store makes what is pending then, and a load reads what is stored then: one right
    // before another of its kind does the same.
    const last = queue[queue.length - 1];
    const entry = { kind, run: () => run(request) };
    if (last?.kind === kind) queue[queue.length - 1] = entry;
    else queue.push(entry);
    if (!busy) next();
  };
  const show = (value: T) => {
    shown = value;
    options.show(value);
  };
  const changed = (value: T, changes = pending) => changes.reduce((value, change) => change(value), value);

  const store = () =>
    enqueue("store", async (request) => {
      const changes = pending.slice();
      if (changes.length === 0) return;
      // Nothing loaded yet (never in the app, which loads first): what is shown has them.
      const value = base === undefined ? shown : changed(base, changes);
      if (value === undefined) return;
      await options.store(value).then(
        (saved) => {
          base = saved;
          pending.splice(0, changes.length);
          if (request === latest && pending.length === 0) show(saved);
        },
        (e: unknown) => {
          if (pending.length > changes.length) return;
          pending.splice(0, changes.length);
          options.failed(e);
          reload();
        },
      );
    });
  const flush = () => {
    if (waiting === null) return;
    clearTimeout(waiting);
    waiting = null;
    store();
  };
  const reload = () =>
    enqueue("load", (request) =>
      options.load().then((stored) => {
        base = stored;
        if (request === latest) show(changed(stored));
      }, console.error),
    );
  return {
    save(change, delay) {
      if (shown === undefined && base === undefined) return;
      pending.push(change);
      show(change(shown ?? (base as T)));
      if (waiting !== null) clearTimeout(waiting);
      waiting = null;
      if (delay === undefined) store();
      else waiting = setTimeout(flush, delay);
    },
    flush,
    reload,
    loaded(value) {
      if (base !== undefined) return;
      base = value;
      show(changed(value));
    },
  };
}
