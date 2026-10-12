import { describe, expect, it } from "vitest";

import { passwordUpdate } from "./password";

describe("passwordUpdate", () => {
  it("deletes a stored password the settings no longer use, if there was one", () => {
    expect(passwordUpdate({ keeps: false, existed: true, clear: false, password: "secret" })).toBe("");
    expect(passwordUpdate({ keeps: false, existed: false, clear: false, password: "secret" })).toBeUndefined();
  });

  it("deletes it when asked, replaces it when one is typed, and keeps it otherwise", () => {
    expect(passwordUpdate({ keeps: true, existed: true, clear: true, password: "typed" })).toBe("");
    expect(passwordUpdate({ keeps: true, existed: true, clear: false, password: "typed" })).toBe("typed");
    expect(passwordUpdate({ keeps: true, existed: false, clear: false, password: "typed" })).toBe("typed");
    expect(passwordUpdate({ keeps: true, existed: true, clear: false, password: "" })).toBeUndefined();
  });
});
