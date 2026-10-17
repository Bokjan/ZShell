import { describe, expect, it } from "vitest";

import { latestOnly } from "./latestOnly";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

describe("latestOnly", () => {
  it("applies only the latest answer, whatever order they come in", async () => {
    const applied: string[] = [];
    const answers = latestOnly<string>((value) => applied.push(value));
    const first = deferred<string>();
    const second = deferred<string>();
    const asked = [answers.track(first.promise), answers.track(second.promise)];
    second.resolve("unlocked");
    first.resolve("locked");
    // Each still resolves to its own answer.
    expect(await Promise.all(asked)).toEqual(["locked", "unlocked"]);
    expect(applied).toEqual(["unlocked"]);
  });

  it("drops an answer asked before a change made here", async () => {
    const applied: string[] = [];
    const answers = latestOnly<string>((value) => applied.push(value));
    const before = deferred<string>();
    const asked = answers.track(before.promise);
    answers.changed("covered");
    before.resolve("uncovered");
    await asked;
    expect(applied).toEqual(["covered"]);
  });

  it("applies the answer to a change over those to questions asked while it was on its way", async () => {
    const applied: string[] = [];
    const answers = latestOnly<string>((value) => applied.push(value));
    const unlocking = deferred<string>();
    const changed = answers.change(unlocking.promise);
    // Asked while unlocking, and answered first, from before the unlock.
    const status = deferred<string>();
    const asked = answers.track(status.promise);
    status.resolve("locked");
    await asked;
    unlocking.resolve("unlocked");
    expect(await changed).toBe("unlocked");
    // A question asked before the change was answered doesn't count after it.
    const late = deferred<string>();
    const lateAsked = answers.track(late.promise);
    answers.outdate();
    late.resolve("locked");
    await lateAsked;
    expect(applied).toEqual(["locked", "unlocked"]);
  });
});
