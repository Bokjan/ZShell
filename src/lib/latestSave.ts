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
  const store = (value: T, request: number) => {
    options.store(value).then(
      (saved) => request === latest && options.show(saved),
      (e: unknown) => {
        if (request !== latest) return;
        options.failed(e);
        reload();
      },
    );
  };
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
    options.load().then((stored) => request === latest && options.show(stored), console.error);
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
