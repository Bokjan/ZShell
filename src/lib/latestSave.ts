/**
 * A value saved whole to the backend (the settings, the quick commands), shown as changed at
 * once. Only the latest request's answer is applied: an older one would undo a newer change,
 * here or in another window.
 *
 * A save that fails puts back the stored value, which the change never reached, and is
 * reported: otherwise the change would stay on screen until the next start. A failure that
 * a newer request has overtaken is neither: that request carries the whole value too.
 *
 * A save can wait (a field saved as it is typed, which would otherwise write the file on each
 * key): the change is shown at once and stored once no other change has come for `delay`
 * ms. Anything else stores it first: a save that doesn't wait, `flush`, and `reload`, so that
 * the last change made here is not lost to a change from another window.
 *
 * One request is with the backend at a time, which keeps them in order (its commands run
 * apart, off the main thread, as each writes a file): the others wait, a newer value taking
 * the place of one waiting to be stored, so that changes made faster than the disk takes them
 * (a number stepped with a key held down) are stored as they can be, the last one always.
 */
export interface LatestSave<T> {
  /** Shows `value` and saves it, after `delay` ms without another change if given. */
  save(value: T, delay?: number): void;
  /** Stores a change still waiting, if any. */
  flush(): void;
  /** Shows the stored value (after a change in another window). */
  reload(): void;
}

export function latestSave<T>(options: {
  show(value: T): void;
  store(value: T): Promise<T>;
  load(): Promise<T>;
  failed(error: unknown): void;
}): LatestSave<T> {
  let latest = 0;
  let waiting: { value: T; timer: ReturnType<typeof setTimeout> } | null = null;
  // The requests after the one with the backend, in order: stores and loads.
  const queue: { kind: "store" | "load"; run(): Promise<void> }[] = [];
  let busy = false;
  const next = () => {
    const request = queue.shift();
    busy = request !== undefined;
    request?.run().then(next, next);
  };
  const enqueue = (kind: "store" | "load", run: () => Promise<void>) => {
    // The value stored is whole: a newer one replaces one waiting right before it.
    const last = queue[queue.length - 1];
    if (last?.kind === kind) queue[queue.length - 1] = { kind, run };
    else queue.push({ kind, run });
    if (!busy) next();
  };
  const store = (value: T, request: number) =>
    enqueue("store", () =>
      options.store(value).then(
        (saved) => {
          if (request === latest) options.show(saved);
        },
        (e: unknown) => {
          if (request !== latest) return;
          options.failed(e);
          reload();
        },
      ),
    );
  const flush = () => {
    if (!waiting) return;
    clearTimeout(waiting.timer);
    const { value } = waiting;
    waiting = null;
    store(value, latest);
  };
  const reload = () => {
    if (waiting) return flush();
    const request = ++latest;
    enqueue("load", () =>
      options.load().then((stored) => {
        if (request === latest) options.show(stored);
      }, console.error),
    );
  };
  return {
    save(value, delay) {
      const request = ++latest;
      options.show(value);
      if (waiting) clearTimeout(waiting.timer);
      waiting = null;
      if (delay === undefined) store(value, request);
      else waiting = { value, timer: setTimeout(flush, delay) };
    },
    flush,
    reload,
  };
}
