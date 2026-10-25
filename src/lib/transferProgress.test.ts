import { describe, expect, it } from "vitest";

import type { TransferProgress } from "../ipc/api";
import { TransferProgresses } from "./transferProgress";

const progress = (transferred: number): TransferProgress => ({ transferred, total: 100, filesDone: 0, filesTotal: 1, current: "a" });

describe("TransferProgresses", () => {
  it("has nothing for a transfer until it reports, then its last report", () => {
    const progresses = new TransferProgresses();
    expect(progresses.get("a")).toBeNull();
    const first = progress(10);
    progresses.set("a", first);
    expect(progresses.get("a")).toBe(first);
    progresses.set("a", progress(20));
    expect(progresses.get("a")?.transferred).toBe(20);
  });

  it("tells only the listeners of the transfer that reported, until they stop", () => {
    const progresses = new TransferProgresses();
    const calls: string[] = [];
    const stopA = progresses.subscribe("a", () => calls.push("a"));
    progresses.subscribe("b", () => calls.push("b"));
    progresses.set("a", progress(1));
    progresses.set("b", progress(1));
    stopA();
    progresses.set("a", progress(2));
    expect(calls).toEqual(["a", "b"]);
  });

  it("forgets the progress of the transfers given", () => {
    const progresses = new TransferProgresses();
    progresses.set("a", progress(1));
    progresses.set("b", progress(1));
    progresses.forget(["a"]);
    expect(progresses.get("a")).toBeNull();
    expect(progresses.get("b")).not.toBeNull();
  });
});
