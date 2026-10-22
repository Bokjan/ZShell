import { describe, expect, it } from "vitest";

import { errorCode, errorMessage, profileForwards, type CommandError, type ForwardRule, type Profile } from "./api";

const error: CommandError = { code: "session.notFound", params: {}, message: "The session no longer exists." };

describe("errorCode and errorMessage", () => {
  it("read a command's error", () => {
    expect(errorCode(error)).toBe("session.notFound");
    expect(errorMessage(error)).toBe("The session no longer exists.");
  });

  it("give no code for anything else, and its text as the message", () => {
    expect(errorCode(new Error("boom"))).toBeNull();
    expect(errorCode("boom")).toBeNull();
    expect(errorCode(null)).toBeNull();
    expect(errorCode({ code: "session.notFound" })).toBeNull();
    expect(errorMessage(new Error("boom"))).toBe("Error: boom");
    expect(errorMessage("boom")).toBe("boom");
  });
});

describe("profileForwards", () => {
  const rule = { id: "r1" } as ForwardRule;

  it("gives an SSH session's rules, and none for other sessions", () => {
    expect(profileForwards({ connection: { protocol: "ssh", forwards: [rule] } } as unknown as Profile)).toEqual([rule]);
    expect(profileForwards({ connection: { protocol: "telnet" } } as unknown as Profile)).toEqual([]);
    expect(profileForwards({ connection: { protocol: "serial" } } as unknown as Profile)).toEqual([]);
  });
});
