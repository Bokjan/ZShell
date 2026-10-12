import { describe, expect, it } from "vitest";

import { nextFieldText } from "./fieldText";

const trimmed = (text: string, value: string) => text.trim() === value;

describe("nextFieldText", () => {
  it("shows a value changed elsewhere", () => {
    expect(nextFieldText("80", "80", "120")).toBe("120");
  });

  it("keeps an edit that isn't saved yet", () => {
    expect(nextFieldText("100", "80", "120")).toBe("100");
  });

  it("keeps the text once its own edit is saved", () => {
    expect(nextFieldText("100", "80", "100")).toBe("100");
  });

  it("keeps what is typed when the value comes back trimmed", () => {
    expect(nextFieldText("session ", "session", "session", trimmed)).toBe("session ");
    expect(nextFieldText("session ", "sess", "session", trimmed)).toBe("session ");
    expect(nextFieldText("session ", "session", "other", trimmed)).toBe("other");
  });
});
