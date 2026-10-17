import { describe, expect, it } from "vitest";

import { createStore } from "./store";

type Action = { type: "add"; n: number } | { type: "same" };
const reduce = (state: { total: number }, action: Action) => (action.type === "add" ? { total: state.total + action.n } : state);

describe("createStore with a reducer", () => {
  it("applies actions and tells listeners", () => {
    const store = createStore({ total: 0 }, reduce);
    let calls = 0;
    store.subscribe(() => calls++);
    store.dispatch({ type: "add", n: 2 });
    store.dispatch({ type: "add", n: 3 });
    expect(store.get()).toEqual({ total: 5 });
    expect(calls).toBe(2);
  });

  it("tells no one of an action that changes nothing", () => {
    const store = createStore({ total: 0 }, reduce);
    const before = store.get();
    let calls = 0;
    store.subscribe(() => calls++);
    store.dispatch({ type: "same" });
    expect(store.get()).toBe(before);
    expect(calls).toBe(0);
  });

  it("stops telling a listener once it unsubscribes", () => {
    const store = createStore({ total: 0 }, reduce);
    let calls = 0;
    const stop = store.subscribe(() => calls++);
    store.dispatch({ type: "add", n: 1 });
    stop();
    store.dispatch({ type: "add", n: 1 });
    expect(calls).toBe(1);
  });

  it("has the new value when a listener is told", () => {
    const store = createStore({ total: 0 }, reduce);
    const seen: number[] = [];
    store.subscribe(() => seen.push(store.get().total));
    store.dispatch({ type: "add", n: 4 });
    expect(seen).toEqual([4]);
  });
});

describe("createStore with set", () => {
  it("changes the fields of a patch, keeping the others", () => {
    const store = createStore({ a: 1, b: "x" });
    let calls = 0;
    store.subscribe(() => calls++);
    store.set({ a: 2 });
    expect(store.get()).toEqual({ a: 2, b: "x" });
    expect(calls).toBe(1);
  });

  it("tells no one of a patch that changes nothing", () => {
    const list = [1];
    const store = createStore({ a: 1, list });
    const before = store.get();
    let calls = 0;
    store.subscribe(() => calls++);
    store.set({ a: 1, list });
    store.set({});
    expect(store.get()).toBe(before);
    expect(calls).toBe(0);
  });

  it("compares fields by identity", () => {
    const store = createStore({ list: [1] });
    let calls = 0;
    store.subscribe(() => calls++);
    store.set({ list: [1] });
    expect(calls).toBe(1);
  });
});
