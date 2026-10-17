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
});
