import { describe, expect, it } from "vitest";

import { clicked, movedTo, NO_SELECTION, pressed, selectOnly, type Selection } from "./fileSelection";

const rows = ["a", "b", "c", "d"];
const none = { shift: false, mod: false };
const paths = (selection: Selection) => [...selection.paths].sort();

describe("file selection", () => {
  it("selects a row, extends with shift and toggles with the modifier", () => {
    let selection = pressed(NO_SELECTION, rows, "b", none);
    expect(selection).toEqual(selectOnly(["b"]));
    selection = pressed(selection, rows, "d", { shift: true, mod: false });
    expect(paths(selection)).toEqual(["b", "c", "d"]);
    expect([selection.anchor, selection.cursor]).toEqual(["b", "d"]);
    selection = pressed(selection, rows, "c", { shift: false, mod: true });
    expect(paths(selection)).toEqual(["b", "d"]);
    // Shift with the modifier adds the range to what is selected.
    selection = pressed(selectOnly(["a"]), rows, "b", { shift: false, mod: true });
    selection = pressed(selection, rows, "d", { shift: true, mod: true });
    expect(paths(selection)).toEqual(["a", "b", "c", "d"]);
  });

  it("keeps a multiple selection while pressed, for dragging, and narrows it on the click", () => {
    const selection = selectOnly(["a", "b", "c"]);
    expect(pressed(selection, rows, "b", none)).toBe(selection);
    expect(clicked(selection, "b", none)).toEqual(selectOnly(["b"]));
    expect(clicked(selection, "b", { shift: true, mod: false })).toBe(selection);
  });

  it("moves the cursor within the list, extending from the anchor", () => {
    expect(movedTo(NO_SELECTION, rows, 9, false)).toEqual(selectOnly(["d"]));
    expect(movedTo(NO_SELECTION, rows, -1, false)).toEqual(selectOnly(["a"]));
    const extended = movedTo(selectOnly(["b"]), rows, 0, true);
    expect([paths(extended), extended.cursor]).toEqual([["a", "b"], "a"]);
    expect(movedTo(NO_SELECTION, [], 0, false)).toBe(NO_SELECTION);
  });
});
