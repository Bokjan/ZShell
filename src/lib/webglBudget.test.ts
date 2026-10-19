import { describe, expect, it } from "vitest";

import { beyondBudget } from "./webglBudget";

const terminal = (name: string, used: number) => ({ name, used });

describe("beyondBudget", () => {
  it("keeps every terminal while they fit", () => {
    expect(beyondBudget([terminal("a", 1), terminal("b", 2)], 2)).toEqual([]);
    expect(beyondBudget([], 0)).toEqual([]);
  });

  it("gives up the least recently used beyond the budget", () => {
    const drawing = [terminal("a", 5), terminal("b", 1), terminal("c", 9), terminal("d", 3)];
    expect(beyondBudget(drawing, 2).map((t) => t.name)).toEqual(["d", "b"]);
    expect(beyondBudget(drawing, 0).map((t) => t.name)).toEqual(["c", "a", "d", "b"]);
  });

  it("leaves the list it is given as it was", () => {
    const drawing = [terminal("a", 1), terminal("b", 2)];
    beyondBudget(drawing, 1);
    expect(drawing.map((t) => t.name)).toEqual(["a", "b"]);
  });
});
