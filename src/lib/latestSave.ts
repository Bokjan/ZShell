/**
 * A value saved whole to the backend (the settings, the quick commands), shown as changed at
 * once. Only the latest request's answer is applied: an older one would undo a newer change,
 * here or in another window.
 *
 * A save that fails puts back the stored value, which the change never reached, and is
 * reported: otherwise the change would stay on screen until the next start. A failure that
 * a newer request has overtaken is neither: that request carries the whole value too.
 */
export interface LatestSave<T> {
  /** Shows `value` and saves it. */
  save(value: T): void;
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
  const reload = () => {
    const request = ++latest;
    options.load().then((stored) => request === latest && options.show(stored), console.error);
  };
  return {
    save(value) {
      const request = ++latest;
      options.show(value);
      options.store(value).then(
        (saved) => request === latest && options.show(saved),
        (e: unknown) => {
          if (request !== latest) return;
          options.failed(e);
          reload();
        },
      );
    },
    reload,
  };
}
