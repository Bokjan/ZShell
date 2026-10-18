import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTabRest, REST_MS } from "./tabRest";

describe("createTabRest", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => void vi.useRealTimers());

  it("shows a tab rested on long enough, and holds it meanwhile", () => {
    const shown: number[] = [];
    const rest = createTabRest(
      () => 1,
      (key) => shown.push(key),
    );
    rest.rest(2);
    expect(rest.get()).toBe(2);
    vi.advanceTimersByTime(REST_MS - 1);
    expect(shown).toEqual([]);
    // Resting on the same tab again doesn't start the time again.
    rest.rest(2);
    vi.advanceTimersByTime(1);
    expect(shown).toEqual([2]);
    expect(rest.get()).toBeNull();
  });

  it("starts again for another tab, and stops when the pointer leaves", () => {
    const shown: number[] = [];
    const rest = createTabRest(
      () => 1,
      (key) => shown.push(key),
    );
    rest.rest(2);
    vi.advanceTimersByTime(REST_MS - 10);
    rest.rest(3);
    vi.advanceTimersByTime(REST_MS - 10);
    rest.rest(null);
    vi.advanceTimersByTime(REST_MS);
    expect(shown).toEqual([]);
    expect(rest.get()).toBeNull();
  });

  it("doesn't rest on the tab shown", () => {
    const shown: number[] = [];
    const rest = createTabRest(
      () => 1,
      (key) => shown.push(key),
    );
    rest.rest(1);
    vi.advanceTimersByTime(REST_MS);
    expect(rest.get()).toBeNull();
    expect(shown).toEqual([]);
  });
});
