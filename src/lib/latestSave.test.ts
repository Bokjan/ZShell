import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { latestSave } from "./latestSave";

/** A promise to settle by hand. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const settle = () => new Promise((done) => setTimeout(done));

function setup(stored: string) {
  const shown: string[] = [];
  const failures: unknown[] = [];
  const stores: ReturnType<typeof deferred<string>>[] = [];
  const saver = latestSave<string>({
    show: (value) => shown.push(value),
    store: () => {
      const request = deferred<string>();
      stores.push(request);
      return request.promise;
    },
    load: () => Promise.resolve(stored),
    failed: (e) => failures.push(e),
  });
  saver.loaded(stored);
  shown.length = 0;
  return { saver, shown, failures, stores };
}

describe("latestSave", () => {
  it("shows a change at once, then what was saved", async () => {
    const { saver, shown, stores } = setup("stored");
    saver.save(() => "a");
    expect(shown).toEqual(["a"]);
    stores[0].resolve("a (saved)");
    await settle();
    expect(shown).toEqual(["a", "a (saved)"]);
  });

  it("puts back the stored value when a save fails, and reports it", async () => {
    const { saver, shown, failures, stores } = setup("stored");
    saver.save(() => "a");
    stores[0].reject("disk full");
    await settle();
    expect(failures).toEqual(["disk full"]);
    expect(shown).toEqual(["a", "stored"]);
  });

  it("stores one value at a time, the newest of those waiting next", async () => {
    const { saver, shown, failures, stores } = setup("stored");
    saver.save(() => "a");
    saver.save(() => "b");
    saver.save(() => "c");
    expect(stores).toHaveLength(1);
    // Overtaken, failed or not: not shown or reported.
    stores[0].reject("disk full");
    await settle();
    expect(stores).toHaveLength(2);
    stores[1].resolve("c (saved)");
    await settle();
    expect(stores).toHaveLength(2);
    expect(failures).toEqual([]);
    expect(shown).toEqual(["a", "b", "c", "c (saved)"]);
  });

  it("reports the newest failure even after older saves succeeded", async () => {
    const { saver, shown, failures, stores } = setup("stored");
    saver.save(() => "a");
    saver.save(() => "b");
    stores[0].resolve("a (saved)");
    await settle();
    stores[1].reject("read-only");
    await settle();
    expect(failures).toEqual(["read-only"]);
    expect(shown).toEqual(["a", "b", "stored"]);
  });

  it("loads after a save on its way, not before", async () => {
    const shown: string[] = [];
    let stored = "stored";
    const saver = latestSave<string>({
      show: (value) => shown.push(value),
      store: async (value) => {
        await settle();
        stored = value;
        return value;
      },
      load: async () => stored,
      failed: () => {},
    });
    saver.loaded(stored);
    shown.length = 0;
    saver.save(() => "a");
    // Another window changed them meanwhile.
    saver.reload();
    await settle();
    await settle();
    expect(shown).toEqual(["a", "a"]);
  });

  it("keeps what another window stored over a first read that comes after it", async () => {
    const shown: string[] = [];
    const saver = latestSave<string>({
      show: (value) => shown.push(value),
      store: async (value) => value,
      load: async () => "changed there",
      failed: () => {},
    });
    // The first read is still on its way when the other window's change is read.
    saver.reload();
    await settle();
    saver.loaded("stored");
    expect(shown).toEqual(["changed there"]);
    saver.save((value) => `${value}, and here`);
    expect(shown[shown.length - 1]).toBe("changed there, and here");
  });

  it("lets a change made after a failure win over putting back", async () => {
    const { saver, shown, stores } = setup("stored");
    saver.save(() => "a");
    stores[0].reject("disk full");
    // Before the stored value arrives.
    saver.save(() => "b");
    await settle();
    expect(shown).toEqual(["a", "b"]);
  });

  it("reloads the stored value unless a change follows", async () => {
    const { saver, shown } = setup("stored");
    saver.reload();
    await settle();
    expect(shown).toEqual(["stored"]);
    saver.reload();
    saver.save(() => "a");
    await settle();
    expect(shown).toEqual(["stored", "a"]);
  });

  describe("waiting", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("shows each change at once and stores the last once changes stop", () => {
      const { saver, shown, stores } = setup("stored");
      saver.save(() => "a", 500);
      vi.advanceTimersByTime(300);
      saver.save(() => "ab", 500);
      vi.advanceTimersByTime(499);
      expect(shown).toEqual(["a", "ab"]);
      expect(stores).toHaveLength(0);
      vi.advanceTimersByTime(1);
      expect(stores).toHaveLength(1);
    });

    it("stores a waiting change when flushed, once", async () => {
      const { saver, shown, stores } = setup("stored");
      saver.save(() => "a", 500);
      saver.flush();
      saver.flush();
      vi.advanceTimersByTime(500);
      expect(stores).toHaveLength(1);
      stores[0].resolve("a (saved)");
      await vi.runAllTimersAsync();
      expect(shown).toEqual(["a", "a (saved)"]);
    });

    it("drops a waiting change that a save overtakes", () => {
      const { saver, stores } = setup("stored");
      saver.save(() => "a", 500);
      saver.save(() => "b");
      vi.advanceTimersByTime(500);
      expect(stores).toHaveLength(1);
    });

    it("keeps a waiting change on top of what another window stored", async () => {
      interface Value {
        here: string;
        there: string;
      }
      let stored: Value = { here: "", there: "" };
      const shown: Value[] = [];
      const saver = latestSave<Value>({
        show: (value) => shown.push(value),
        store: async (value) => (stored = value),
        load: async () => stored,
        failed: () => {},
      });
      saver.loaded(stored);
      saver.save((value) => ({ ...value, here: "typed" }), 500);
      // Another window stores its change while this one waits.
      stored = { ...stored, there: "changed" };
      saver.reload();
      await vi.advanceTimersByTimeAsync(0);
      expect(shown[shown.length - 1]).toEqual({ here: "typed", there: "changed" });
      await vi.advanceTimersByTimeAsync(500);
      expect(stored).toEqual({ here: "typed", there: "changed" });
    });
  });
});
