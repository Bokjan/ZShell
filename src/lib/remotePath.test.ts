import { describe, expect, it } from "vitest";

import { joinPath, parentPath } from "./remotePath";

describe("joinPath", () => {
  it("adds a slash only where there is none", () => {
    expect(joinPath("/home/me", "a.txt")).toBe("/home/me/a.txt");
    expect(joinPath("/home/me/", "a.txt")).toBe("/home/me/a.txt");
    expect(joinPath("/", "etc")).toBe("/etc");
  });
});

describe("parentPath", () => {
  it("drops the last name, with a trailing slash or without", () => {
    expect(parentPath("/home/me/a.txt")).toBe("/home/me");
    expect(parentPath("/home/me/")).toBe("/home");
  });

  it("stops at the root", () => {
    expect(parentPath("/etc")).toBe("/");
    expect(parentPath("/")).toBe("/");
  });
});
