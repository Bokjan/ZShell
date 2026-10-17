import { latestSave, type LatestSave } from "./latestSave";
import { createStore, type Store } from "./store";

export interface SavedState<T> {
  /** Null until it is read. */
  value: T | null;
  /** Why the last change couldn't be saved (it was undone), until dismissed. */
  saveError: unknown;
}

/**
 * A value saved whole to the backend (the settings, the quick commands) as a store: changed
 * here as `latestSave` changes it, shown at once, and read by any component or module rather
 * than passed down. One per window.
 */
export interface SavedStore<T> extends Store<SavedState<T>>, LatestSave<T> {
  dismissSaveError(): void;
  /** Forgets the value, and any answer still to come (the app is unmounted: the vault locked). */
  clear(): void;
}

export function createSavedStore<T>(options: { store(value: T): Promise<T>; load(): Promise<T> }): SavedStore<T> {
  const state = createStore<SavedState<T>>({ value: null, saveError: null });
  // A new one after `clear`: the old one's answers are dropped.
  const make = (): LatestSave<T> => {
    const saver: LatestSave<T> = latestSave<T>({
      show: (value) => saver === current && state.set({ value }),
      store: options.store,
      load: options.load,
      failed: (saveError) => saver === current && state.set({ saveError }),
    });
    return saver;
  };
  let current = make();
  return {
    get: state.get,
    subscribe: state.subscribe,
    save: (change, delay) => current.save(change, delay),
    flush: () => current.flush(),
    reload: () => current.reload(),
    loaded: (value) => current.loaded(value),
    dismissSaveError: () => state.set({ saveError: null }),
    clear() {
      current = make();
      state.set({ value: null, saveError: null });
    },
  };
}
