import { describe, expect, it } from "vitest";

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
  return { saver, shown, failures, stores };
}

describe("latestSave", () => {
  it("shows a change at once, then what was saved", async () => {
    const { saver, shown, stores } = setup("stored");
    saver.save("a");
    expect(shown).toEqual(["a"]);
    stores[0].resolve("a (saved)");
    await settle();
    expect(shown).toEqual(["a", "a (saved)"]);
  });

  it("puts back the stored value when a save fails, and reports it", async () => {
    const { saver, shown, failures, stores } = setup("stored");
    saver.save("a");
    stores[0].reject("disk full");
    await settle();
    expect(failures).toEqual(["disk full"]);
    expect(shown).toEqual(["a", "stored"]);
  });

  it("ignores answers that a newer save overtook, failed or not", async () => {
    const { saver, shown, failures, stores } = setup("stored");
    saver.save("a");
    saver.save("b");
    saver.save("c");
    stores[0].reject("disk full");
    stores[1].resolve("b (saved)");
    stores[2].resolve("c (saved)");
    await settle();
    expect(failures).toEqual([]);
    expect(shown).toEqual(["a", "b", "c", "c (saved)"]);
  });

  it("reports the newest failure even after older saves succeeded", async () => {
    const { saver, shown, failures, stores } = setup("stored");
    saver.save("a");
    saver.save("b");
    stores[0].resolve("a (saved)");
    stores[1].reject("read-only");
    await settle();
    expect(failures).toEqual(["read-only"]);
    expect(shown).toEqual(["a", "b", "stored"]);
  });

  it("lets a change made after a failure win over putting back", async () => {
    const { saver, shown, stores } = setup("stored");
    saver.save("a");
    stores[0].reject("disk full");
    // Before the stored value arrives.
    saver.save("b");
    await settle();
    expect(shown).toEqual(["a", "b"]);
  });

  it("reloads the stored value unless a change follows", async () => {
    const { saver, shown } = setup("stored");
    saver.reload();
    await settle();
    expect(shown).toEqual(["stored"]);
    saver.reload();
    saver.save("a");
    await settle();
    expect(shown).toEqual(["stored", "a"]);
  });
});
