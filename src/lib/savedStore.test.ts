import { describe, expect, it } from "vitest";

import { createSavedStore } from "./savedStore";

const settle = () => new Promise((done) => setTimeout(done));

function setup(stored: string, store: (value: string) => Promise<string> = (value) => Promise.resolve(value)) {
  const saved = createSavedStore<string>({ store, load: () => Promise.resolve(stored) });
  let calls = 0;
  saved.subscribe(() => calls++);
  return { saved, calls: () => calls };
}

describe("createSavedStore", () => {
  it("has no value until it is read", () => {
    const { saved } = setup("stored");
    expect(saved.get()).toEqual({ value: null, saveError: null });
  });

  it("shows what is read and a change made to it", async () => {
    const { saved, calls } = setup("stored");
    saved.loaded("stored");
    expect(saved.get().value).toBe("stored");
    saved.save((value) => `${value}+a`);
    expect(saved.get().value).toBe("stored+a");
    await settle();
    expect(saved.get().value).toBe("stored+a");
    // Saving what is shown already tells no one again.
    expect(calls()).toBe(2);
  });

  it("puts back the stored value and keeps the error when a save fails", async () => {
    const error = new Error("disk full");
    const { saved } = setup("stored", () => Promise.reject(error));
    saved.loaded("stored");
    saved.save(() => "changed");
    await settle();
    expect(saved.get()).toEqual({ value: "stored", saveError: error });
    saved.dismissSaveError();
    expect(saved.get().saveError).toBeNull();
  });

  it("drops answers that come after it is cleared", async () => {
    let finish!: (value: string) => void;
    const { saved } = setup("stored", () => new Promise((resolve) => (finish = resolve)));
    saved.loaded("stored");
    saved.save(() => "changed");
    saved.clear();
    expect(saved.get()).toEqual({ value: null, saveError: null });
    finish("changed");
    await settle();
    expect(saved.get().value).toBeNull();
    saved.loaded("again");
    expect(saved.get().value).toBe("again");
  });
});
