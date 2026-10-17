/**
 * A value kept outside React that components read with `useSyncExternalStore` (see
 * `hooks/stores.ts`): the tabs, the session directory, the settings and so on. Listeners hear
 * of a change only when the value is a different one, so a change that changes nothing renders
 * nothing again.
 */
export interface Store<S> {
  /** The current value, also between a change and the next render. */
  get(): S;
  /** Calls `listener` after each change; returns what stops it. */
  subscribe(listener: () => void): () => void;
}

/** A store changed by actions, which a pure function turns into the next value. */
export interface ReducerStore<S, A> extends Store<S> {
  dispatch(action: A): void;
}

/** A store of an object changed a few fields at a time. */
export interface PatchStore<S extends object> extends Store<S> {
  /** Changes the fields of `patch`; listeners hear of it only if any of them is different. */
  set(patch: Partial<S>): void;
}

/** A store whose actions `reduce` applies; an action that returns the same value changes nothing. */
export function createStore<S, A>(initial: S, reduce: (state: S, action: A) => S): ReducerStore<S, A>;
/** A store changed with `set(patch)`. */
export function createStore<S extends object>(initial: S): PatchStore<S>;
export function createStore<S, A>(initial: S, reduce?: (state: S, action: A) => S): ReducerStore<S, A> & PatchStore<S & object> {
  let state = initial;
  const listeners = new Set<() => void>();
  const replace = (next: S) => {
    if (next === state) return;
    state = next;
    for (const listener of listeners) listener();
  };
  return {
    get: () => state as S & object,
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    dispatch: (action) => replace(reduce!(state, action)),
    set(patch) {
      const fields = Object.keys(patch) as (keyof S)[];
      if (fields.every((field) => Object.is(patch[field], state[field]))) return;
      replace({ ...state, ...patch });
    },
  };
}
