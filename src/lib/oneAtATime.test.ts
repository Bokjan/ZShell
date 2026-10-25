import { describe, expect, it } from "vitest";

import { oneAtATime } from "./oneAtATime";

/** A run of the work, which ends when told to. */
const runs = () => {
  const ends: (() => void)[] = [];
  const work = () => new Promise<void>((resolve) => ends.push(resolve));
  return { ends, work };
};

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("oneAtATime", () => {
  it("runs once for an event while nothing runs", async () => {
    const { ends, work } = runs();
    const event = oneAtATime(work);
    event();
    expect(ends).toHaveLength(1);
    ends[0]();
    await settle();
    expect(ends).toHaveLength(1);
  });

  it("runs once more after a run, for all the events that came while it ran", async () => {
    const { ends, work } = runs();
    const event = oneAtATime(work);
    event();
    event();
    event();
    event();
    expect(ends).toHaveLength(1);
    ends[0]();
    await settle();
    expect(ends).toHaveLength(2);
    ends[1]();
    await settle();
    expect(ends).toHaveLength(2);
  });

  it("goes on after a run that failed", async () => {
    let calls = 0;
    const event = oneAtATime(() => {
      calls++;
      return Promise.reject(new Error("no"));
    });
    event();
    await settle();
    event();
    await settle();
    expect(calls).toBe(2);
  });
});
