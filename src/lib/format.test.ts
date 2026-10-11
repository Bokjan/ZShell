import { describe, expect, it } from "vitest";

import { formatAgo } from "./format";

describe("formatAgo", () => {
  it("says how long ago in the largest whole unit", () => {
    expect(formatAgo(0)).toBe("0 seconds ago");
    expect(formatAgo(1999)).toBe("1 second ago");
    expect(formatAgo(59_999)).toBe("59 seconds ago");
    expect(formatAgo(60_000)).toBe("1 minute ago");
    expect(formatAgo(2 * 3600_000 + 5)).toBe("2 hours ago");
    expect(formatAgo(3 * 86400_000)).toBe("3 days ago");
  });
});
