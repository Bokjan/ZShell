import { describe, expect, it } from "vitest";

import { attempt, attemptSync } from "./attempt";

describe("attempt", () => {
  it("returns what the body returns, then cleans up", async () => {
    const steps: string[] = [];
    const result = await attempt(
      async () => {
        steps.push("body");
        return 1;
      },
      { onError: () => 2, cleanup: () => steps.push("cleanup") },
    );
    expect(result).toBe(1);
    expect(steps).toEqual(["body", "cleanup"]);
  });

  it("returns what onError returns when the body fails, then cleans up", async () => {
    const steps: string[] = [];
    const result = await attempt(
      async () => {
        throw new Error("no");
      },
      {
        onError: async (e) => {
          steps.push(`error ${(e as Error).message}`);
          return 2;
        },
        cleanup: () => steps.push("cleanup"),
      },
    );
    expect(result).toBe(2);
    expect(steps).toEqual(["error no", "cleanup"]);
  });

  it("passes the error on after cleaning up when there is no onError", async () => {
    const steps: string[] = [];
    const failed = attempt(
      async () => {
        throw new Error("no");
      },
      { cleanup: () => steps.push("cleanup") },
    );
    await expect(failed).rejects.toThrow("no");
    expect(steps).toEqual(["cleanup"]);
  });

  it("passes on an error from onError after cleaning up", async () => {
    const steps: string[] = [];
    const failed = attempt(
      async () => {
        throw new Error("first");
      },
      {
        onError: () => {
          throw new Error("second");
        },
        cleanup: () => steps.push("cleanup"),
      },
    );
    await expect(failed).rejects.toThrow("second");
    expect(steps).toEqual(["cleanup"]);
  });

  it("cleans up only once the body has settled", async () => {
    const steps: string[] = [];
    let finish!: () => void;
    const done = attempt(() => new Promise<void>((resolve) => (finish = resolve)), { cleanup: () => steps.push("cleanup") });
    await Promise.resolve();
    expect(steps).toEqual([]);
    finish();
    await done;
    expect(steps).toEqual(["cleanup"]);
  });
});

describe("attemptSync", () => {
  it("returns what the body returns, then cleans up", () => {
    const steps: string[] = [];
    expect(
      attemptSync(
        () => {
          steps.push("body");
          return 1;
        },
        () => steps.push("cleanup"),
      ),
    ).toBe(1);
    expect(steps).toEqual(["body", "cleanup"]);
  });

  it("passes the error on after cleaning up", () => {
    const steps: string[] = [];
    expect(() =>
      attemptSync(
        () => {
          throw new Error("no");
        },
        () => steps.push("cleanup"),
      ),
    ).toThrow("no");
    expect(steps).toEqual(["cleanup"]);
  });
});
