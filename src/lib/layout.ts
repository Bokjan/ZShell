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

/** The layout with each pane's key changed by `f`. */
export const mapLayout = (layout: Layout, f: (key: number) => number): Layout =>
  layout.kind === "pane" ? leaf(f(layout.key)) : { ...layout, children: layout.children.map((child) => mapLayout(child, f)) };

/** The part of `rect` a split's child gets, `offset` and `size` being fractions of the split. */
const childRect = (rect: Rect, direction: Direction, offset: number, size: number): Rect =>
  direction === "row"
    ? { x: rect.x + offset * rect.w, y: rect.y, w: size * rect.w, h: rect.h }
    : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: size * rect.h };

/** A pane is kept at least this large, in pixels: dragging a divider, and splitting it. */
export const MIN_PANE_WIDTH = 120;
export const MIN_PANE_HEIGHT = 60;

export interface Size {
  width: number;
  height: number;
}

/**
 * Whether pane `key` of `layout`, in a pane area of `area` pixels, is large enough to split in
 * two along `direction`, each half at least the minimum size. Unknown sizes allow it.
 */
export function canSplit(layout: Layout, key: number, area: Size | undefined, direction: Direction): boolean {
  const rect = paneRects(layout).get(key);
  if (!rect || !area) return true;
  return direction === "row" ? rect.w * area.width >= 2 * MIN_PANE_WIDTH : rect.h * area.height >= 2 * MIN_PANE_HEIGHT;
}

/** The direction of a split with a pane on `side` of another. */
export const directionOf = (side: Side): Direction => (side === "left" || side === "right" ? "row" : "column");

/**
 * Puts `added` (a pane, or a tab's panes) on `side` of pane `key`, the two sharing the space
 * the pane had. Within a split of the same direction, `added` becomes a sibling of the pane,
 * and so do its own children when it is split that way too.
 */
export function insertLayout(layout: Layout, key: number, side: Side, added: Layout): Layout {
  const direction = directionOf(side);
  const first = side === "left" || side === "up";
  // What `added` brings to a split of `direction` taking `share` of it, with their shares.
  const parts = (share: number): [Layout[], number[]] =>
    added.kind === "split" && added.direction === direction
      ? [added.children, added.sizes.map((size) => size * share)]
      : [[added], [share]];
  if (layout.kind === "pane") {
    if (layout.key !== key) return layout;
    const [children, sizes] = parts(0.5);
    return first
      ? { kind: "split", direction, children: [...children, layout], sizes: [...sizes, 0.5] }
      : { kind: "split", direction, children: [layout, ...children], sizes: [0.5, ...sizes] };
  }
  const index = layout.children.findIndex((child) => child.kind === "pane" && child.key === key);
  if (index >= 0 && layout.direction === direction) {
    const half = layout.sizes[index] / 2;
    const [children, sizes] = parts(half);
    const own = layout.sizes.map((size, i) => (i === index ? half : size));
    const at = first ? index : index + 1;
    return {
      ...layout,
      children: [...layout.children.slice(0, at), ...children, ...layout.children.slice(at)],
      sizes: [...own.slice(0, at), ...sizes, ...own.slice(at)],
    };
  }
  return { ...layout, children: layout.children.map((child) => insertLayout(child, key, side, added)) };
}

/**
 * Splits pane `key` in two along `direction`, putting pane `added` right of or below it.
 * Within a split of the same direction the new pane becomes a sibling, sharing the space
 * the pane had.
 */
export const splitLayout = (layout: Layout, key: number, direction: Direction, added: number): Layout =>
  insertLayout(layout, key, direction === "row" ? "right" : "down", leaf(added));

/**
 * Whether panes `keys` of `layout`, in a pane area of `area` pixels, are each at least the
 * minimum size. Unknown sizes allow it.
 */
export function fits(layout: Layout, keys: number[], area: Size | undefined): boolean {
  if (!area) return true;
  const rects = paneRects(layout);
  return keys.every((key) => {
    const rect = rects.get(key);
    return !rect || (rect.w * area.width >= MIN_PANE_WIDTH - EPSILON && rect.h * area.height >= MIN_PANE_HEIGHT - EPSILON);
  });
}

/**
 * The side a pane added next to pane `key` goes on by itself: right of it when the pane is
 * at least as wide as it is tall (in pixels), else below, or the other if only that has room;
 * null when neither has.
 */
export function autoSide(layout: Layout, key: number, area: Size | undefined): "right" | "down" | null {
  const rect = paneRects(layout).get(key);
  if (!rect) return null;
  const wide = !area || rect.w * area.width >= rect.h * area.height;
  const sides = wide ? (["right", "down"] as const) : (["down", "right"] as const);
  return sides.find((side) => canSplit(layout, key, area, directionOf(side))) ?? null;
}

/**
 * The side of `rect` nearest the point (`x`, `y`), all in the same units: the rect is cut
 * along its diagonals, as where a pane dropped on another one goes.
 */
export function sideAt(rect: Rect, x: number, y: number): Side {
  // Where the point is, from -1 to 1 across the rect each way.
  const dx = ((x - rect.x) / rect.w) * 2 - 1;
  const dy = ((y - rect.y) / rect.h) * 2 - 1;
  if (Math.abs(dx) >= Math.abs(dy)) return dx < 0 ? "left" : "right";
  return dy < 0 ? "up" : "down";
}

/** The part of `rect` that a pane put on `side` of it takes: its half on that side. */
export function halfOn(rect: Rect, side: Side): Rect {
  if (side === "left") return { ...rect, w: rect.w / 2 };
  if (side === "right") return { ...rect, x: rect.x + rect.w / 2, w: rect.w / 2 };
  if (side === "up") return { ...rect, h: rect.h / 2 };
  return { ...rect, y: rect.y + rect.h / 2, h: rect.h / 2 };
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
export const paneKeys = (layout: Layout): number[] => (layout.kind === "pane" ? [layout.key] : layout.children.flatMap(paneKeys));

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
function updateSplit(layout: Layout, path: number[], change: (split: Extract<Layout, { kind: "split" }>) => Layout): Layout {
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
