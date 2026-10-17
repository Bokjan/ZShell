import { describe, expect, it } from "vitest";

import { changedReferences } from "./stableProps";

describe("changedReferences", () => {
  const onOpen = () => {};
  const handlers = { onOpen };

  it("finds nothing when the functions and listed objects are the same", () => {
    expect(changedReferences({ onOpen, handlers, count: 1 }, { onOpen, handlers, count: 2 }, ["handlers"])).toEqual([]);
  });

  it("finds a function that is a new one", () => {
    expect(changedReferences({ onOpen }, { onOpen: () => {} })).toEqual(["onOpen"]);
  });

  it("finds a listed object that is a new one, even with the same contents", () => {
    expect(changedReferences({ handlers }, { handlers: { onOpen } }, ["handlers"])).toEqual(["handlers"]);
  });

  it("leaves out objects that aren't listed and values that aren't functions", () => {
    expect(changedReferences({ tab: { key: 1 }, title: "a" }, { tab: { key: 1 }, title: "b" })).toEqual([]);
  });

  it("finds a function prop that appears or goes away", () => {
    expect(changedReferences({}, { onOpen })).toEqual(["onOpen"]);
    expect(changedReferences({ onOpen }, {})).toEqual(["onOpen"]);
  });
});
