/**
 * The file list's selection, as a file manager's: a click selects one row, ⇧ extends from the
 * anchor, ⌘ / Ctrl toggles a row, and the arrow keys move the cursor (with ⇧, extending).
 * Rows are given by path, in the listed order.
 */
export interface Selection {
  paths: ReadonlySet<string>;
  /** Where a ⇧ range starts. */
  anchor: string | null;
  /** The row the arrow keys move from. */
  cursor: string | null;
}

export const NO_SELECTION: Selection = { paths: new Set(), anchor: null, cursor: null };

/** Exactly `paths`: the anchor at the first (or `anchor`), the cursor at the last. */
export const selectOnly = (paths: string[], anchor: string | null = paths[0] ?? null): Selection => ({
  paths: new Set(paths),
  anchor,
  cursor: paths.length > 0 ? paths[paths.length - 1] : null,
});

/** The paths from `from` to `to` (in either direction) in `rows`' order. */
export function range(rows: string[], from: string, to: string): string[] {
  const a = rows.indexOf(from);
  const b = rows.indexOf(to);
  if (a < 0 || b < 0) return [to];
  return rows.slice(Math.min(a, b), Math.max(a, b) + 1);
}

export interface Modifiers {
  shift: boolean;
  /** ⌘ on macOS, Ctrl elsewhere. */
  mod: boolean;
}

/**
 * A mouse press on row `path`. Without modifiers, a press on a selected row keeps the
 * selection (so that it can be dragged together; see `clicked`).
 */
export function pressed(selection: Selection, rows: string[], path: string, { shift, mod }: Modifiers): Selection {
  if (shift && selection.anchor) {
    const extended = range(rows, selection.anchor, path);
    return { ...selection, paths: new Set(mod ? [...selection.paths, ...extended] : extended), cursor: path };
  }
  if (mod) {
    const paths = new Set(selection.paths);
    if (!paths.delete(path)) paths.add(path);
    return { paths, anchor: path, cursor: path };
  }
  return selection.paths.has(path) ? selection : selectOnly([path]);
}

/** A click (the button released) on row `path` without modifiers: that row alone. */
export const clicked = (selection: Selection, path: string, { shift, mod }: Modifiers): Selection =>
  !shift && !mod && selection.paths.size > 1 ? selectOnly([path]) : selection;

/** The cursor moved to row `index` (kept within the list); with `extend`, from the anchor. */
export function movedTo(selection: Selection, rows: string[], index: number, extend: boolean): Selection {
  if (rows.length === 0) return selection;
  const path = rows[Math.max(0, Math.min(rows.length - 1, index))];
  if (extend && selection.anchor) return { ...selection, paths: new Set(range(rows, selection.anchor, path)), cursor: path };
  return selectOnly([path]);
}
