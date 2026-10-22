import { describe, expect, it } from "vitest";

import { pastThreshold } from "./windowDrag";

// Only what it reads.
const at = (clientX: number, clientY: number) => ({ clientX, clientY }) as MouseEvent;

describe("pastThreshold", () => {
  const grab = { x: 100, y: 100 };

  it("waits for the pointer to move 5 pixels, any way", () => {
    expect(pastThreshold(grab, at(100, 100))).toBe(false);
    expect(pastThreshold(grab, at(103, 103))).toBe(false);
    expect(pastThreshold(grab, at(105, 100))).toBe(true);
    expect(pastThreshold(grab, at(96, 97))).toBe(true);
  });

  it("counts a move out of the window", () => {
    expect(pastThreshold(grab, at(100, -20))).toBe(true);
  });
});
