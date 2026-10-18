import { createStore, type Store } from "./store";

/** How long a dragged tab or pane rests on a tab before that tab is shown (see `.tab.resting`). */
export const REST_MS = 300;

/**
 * A tab or a pane dragged over the tab bar resting on a tab, which is shown after `REST_MS`
 * so that the dragged one can be dropped on its panes. The store holds the tab it rests on,
 * which the tab bar marks with the time left meanwhile.
 */
export interface TabRest extends Store<number | null> {
  /** Rests on tab `key`, or on none (null): the time starts again for another tab. */
  rest(key: number | null): void;
}

/** A `TabRest` that shows a tab with `show`, unless `active` says it is shown already. */
export function createTabRest(active: () => number | null, show: (key: number) => void): TabRest {
  const store = createStore<number | null, number | null>(null, (_, key) => key);
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    get: store.get,
    subscribe: store.subscribe,
    rest(key) {
      const on = key === active() ? null : key;
      if (on === store.get()) return;
      clearTimeout(timer);
      store.dispatch(on);
      if (on === null) return;
      timer = setTimeout(() => {
        store.dispatch(null);
        show(on);
      }, REST_MS);
    },
  };
}
