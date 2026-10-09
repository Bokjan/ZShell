import type { Layout } from "./panes";

/** `row` puts panes side by side, `column` stacks them. */
export type Direction = "row" | "column";

/** Where to move the focus from a pane. */
export type Side = "left" | "right" | "up" | "down";

/** A pane's or a split's place in the tab, as fractions of the tab's pane area. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A divider between two children of a split: `index` is the child after it. */
export interface Divider {
  /** Child indexes from the root down to the split. */
  path: number[];
  index: number;
  direction: Direction;
  /** The split's rect. */
  rect: Rect;
  /** Where the divider is, as a fraction of the split along its direction. */
  at: number;
}

const FULL: Rect = { x: 0, y: 0, w: 1, h: 1 };
/** Rounding slack when comparing pane edges. */
const EPSILON = 1e-6;

const leaf = (key: number): Layout => ({ kind: "pane", key });

/** The part of `rect` a split's child gets, `offset` and `size` being fractions of the split. */
const childRect = (rect: Rect, direction: Direction, offset: number, size: number): Rect =>
  direction === "row"
    ? { x: rect.x + offset * rect.w, y: rect.y, w: size * rect.w, h: rect.h }
    : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: size * rect.h };

/**
 * Splits pane `key` in two along `direction`, putting pane `added` right of or below it.
 * Within a split of the same direction the new pane becomes a sibling, sharing the space
 * the pane had.
 */
export function splitLayout(layout: Layout, key: number, direction: Direction, added: number): Layout {
  if (layout.kind === "pane") {
    return layout.key === key ? { kind: "split", direction, children: [layout, leaf(added)], sizes: [0.5, 0.5] } : layout;
  }
  const index = layout.children.findIndex((child) => child.kind === "pane" && child.key === key);
  if (index >= 0 && layout.direction === direction) {
    const half = layout.sizes[index] / 2;
    return {
      ...layout,
      children: [...layout.children.slice(0, index + 1), leaf(added), ...layout.children.slice(index + 1)],
      sizes: [...layout.sizes.slice(0, index), half, half, ...layout.sizes.slice(index + 1)],
    };
  }
  return { ...layout, children: layout.children.map((child) => splitLayout(child, key, direction, added)) };
}

/**
 * Removes pane `key`, giving its space to the child before it (the one after it if it was
 * first). A split left with one child is replaced by it, and one left inside a split of the
 * same direction merges into it. Null when no pane is left.
 */
export function removeFromLayout(layout: Layout, key: number): Layout | null {
  if (layout.kind === "pane") return layout.key === key ? null : layout;
  const children: Layout[] = [];
  const sizes: number[] = [];
  // Space of a removed first child, for the next child kept.
  let pending = 0;
  layout.children.forEach((child, i) => {
    const kept = removeFromLayout(child, key);
    if (!kept) {
      if (sizes.length > 0) sizes[sizes.length - 1] += layout.sizes[i];
      else pending += layout.sizes[i];
      return;
    }
    if (kept.kind === "split" && kept.direction === layout.direction) {
      // A split whose child was removed, left with a split of this direction: merge it.
      kept.children.forEach((grandchild, j) => {
        children.push(grandchild);
        sizes.push(kept.sizes[j] * layout.sizes[i] + (j === 0 ? pending : 0));
      });
    } else {
      children.push(kept);
      sizes.push(layout.sizes[i] + pending);
    }
    pending = 0;
  });
  if (children.length === 0) return null;
  if (children.length === 1) return children[0];
  return { ...layout, children, sizes };
}

/** The pane next to `key` that gets its space when it is removed (see `removeFromLayout`). */
export function heirOf(layout: Layout, key: number): number | null {
  if (layout.kind === "pane") return null;
  const index = layout.children.findIndex((child) => child.kind === "pane" && child.key === key);
  if (index < 0) {
    for (const child of layout.children) {
      const heir = heirOf(child, key);
      if (heir !== null) return heir;
    }
    return null;
  }
  // The pane of the neighbor nearest the removed one.
  if (index > 0) return paneKeys(layout.children[index - 1]).slice(-1)[0];
  return paneKeys(layout.children[index + 1])[0] ?? null;
}

/** The panes in the layout, left to right and top to bottom. */
export const paneKeys = (layout: Layout): number[] =>
  layout.kind === "pane" ? [layout.key] : layout.children.flatMap(paneKeys);

/** Where each pane is. */
export function paneRects(layout: Layout, rect = FULL, rects = new Map<number, Rect>()): Map<number, Rect> {
  if (layout.kind === "pane") return rects.set(layout.key, rect);
  let offset = 0;
  layout.children.forEach((child, i) => {
    paneRects(child, childRect(rect, layout.direction, offset, layout.sizes[i]), rects);
    offset += layout.sizes[i];
  });
  return rects;
}

/** The dividers between the children of every split. */
export function dividers(layout: Layout, path: number[] = [], rect = FULL, list: Divider[] = []): Divider[] {
  if (layout.kind === "pane") return list;
  let offset = 0;
  layout.children.forEach((child, i) => {
    if (i > 0) list.push({ path, index: i, direction: layout.direction, rect, at: offset });
    dividers(child, [...path, i], childRect(rect, layout.direction, offset, layout.sizes[i]), list);
    offset += layout.sizes[i];
  });
  return list;
}

/** Replaces the split at `path` with `change(split)`. */
function updateSplit(
  layout: Layout,
  path: number[],
  change: (split: Extract<Layout, { kind: "split" }>) => Layout,
): Layout {
  if (layout.kind === "pane") return layout;
  if (path.length === 0) return change(layout);
  const [first, ...rest] = path;
  return { ...layout, children: layout.children.map((child, i) => (i === first ? updateSplit(child, rest, change) : child)) };
}

/**
 * Moves a divider to `at` (a fraction of its split), keeping the two children next to it at
 * least `min` (also a fraction of the split) each.
 */
export function moveDivider(layout: Layout, divider: Divider, at: number, min: number): Layout {
  return updateSplit(layout, divider.path, (split) => {
    const sizes = [...split.sizes];
    const i = divider.index;
    const start = sizes.slice(0, i - 1).reduce((sum, size) => sum + size, 0);
    const pair = sizes[i - 1] + sizes[i];
    const first = pair < 2 * min ? pair / 2 : Math.min(pair - min, Math.max(min, at - start));
    sizes[i - 1] = first;
    sizes[i] = pair - first;
    return { ...split, sizes };
  });
}

/** Gives the children of the split at `path` equal space. */
export const equalize = (layout: Layout, path: number[]) =>
  updateSplit(layout, path, (split) => ({ ...split, sizes: split.children.map(() => 1 / split.children.length) }));

/**
 * The pane beyond `key`'s edge on `side`: of those touching that edge, the one nearest the
 * middle of the edge.
 */
export function neighbor(layout: Layout, key: number, side: Side): number | null {
  const rects = paneRects(layout);
  const from = rects.get(key);
  if (!from) return null;
  const horizontal = side === "left" || side === "right";
  let best: { key: number; distance: number; offset: number } | null = null;
  for (const [other, rect] of rects) {
    if (other === key) continue;
    const distance =
      side === "left"
        ? from.x - (rect.x + rect.w)
        : side === "right"
          ? rect.x - (from.x + from.w)
          : side === "up"
            ? from.y - (rect.y + rect.h)
            : rect.y - (from.y + from.h);
    if (distance < -EPSILON) continue;
    // Along the edge: they must overlap, and the nearer to the edge's middle the better.
    const [a, aSize, b, bSize] = horizontal ? [from.y, from.h, rect.y, rect.h] : [from.x, from.w, rect.x, rect.w];
    if (Math.min(a + aSize, b + bSize) - Math.max(a, b) <= EPSILON) continue;
    const offset = Math.abs(b + bSize / 2 - (a + aSize / 2));
    if (!best || distance < best.distance - EPSILON || (Math.abs(distance - best.distance) <= EPSILON && offset < best.offset)) {
      best = { key: other, distance, offset };
    }
  }
  return best?.key ?? null;
}
