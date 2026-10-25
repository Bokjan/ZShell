/** Something that lets go of what it holds when disposed, as xterm.js's `IDisposable`. */
export interface Disposable {
  dispose(): void;
}

/** A timeout that starts over: one wait at a time, cleared once disposed. */
export class Timer implements Disposable {
  private timeout: ReturnType<typeof setTimeout> | undefined;

  /** Runs `f` in `ms`, instead of what was waiting. */
  start(f: () => void, ms: number) {
    clearTimeout(this.timeout);
    this.timeout = setTimeout(() => {
      this.timeout = undefined;
      f();
    }, ms);
  }

  /** Whatever was waiting doesn't run. */
  clear() {
    clearTimeout(this.timeout);
    this.timeout = undefined;
  }

  dispose() {
    this.clear();
  }
}

/**
 * What something holds while it lives (listeners, subscriptions, timers), let go of together,
 * the last added first; added after that, let go of at once.
 */
export class Disposables implements Disposable {
  private held: (() => void)[] = [];
  private disposed = false;

  /** Disposes `item` with the rest; returns it. */
  add<T extends Disposable>(item: T): T {
    this.defer(() => item.dispose());
    return item;
  }

  /** Runs `f` when disposed. */
  defer(f: () => void) {
    if (this.disposed) f();
    else this.held.push(f);
  }

  /** Listens to `target` until disposed. */
  listen<E extends Event>(target: EventTarget, type: string, listener: (e: E) => void, options?: boolean | AddEventListenerOptions) {
    const handler = listener as EventListener;
    target.addEventListener(type, handler, options);
    this.defer(() => target.removeEventListener(type, handler, options));
  }

  /** A timer cleared when disposed. */
  timer(): Timer {
    return this.add(new Timer());
  }

  dispose() {
    this.disposed = true;
    const held = this.held;
    this.held = [];
    for (let i = held.length - 1; i >= 0; i--) held[i]();
  }
}
