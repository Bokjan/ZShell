/** Part of a long list: rows `start` to `end` drawn, or `rows` rows' height left empty. */
export type Segment = { kind: "rows"; start: number; end: number } | { kind: "gap"; rows: number };

/**
 * A list of `count` rows as drawn: those from `start` to `end` (the part in view, and some
 * beyond), and each of `keep` (rows that must be there wherever the list is scrolled, such as
 * the one screen readers are pointed at), with gaps between.
 */
export function rowSegments(count: number, start: number, end: number, keep: number[]): Segment[] {
  const ranges = [[Math.max(0, start), Math.min(count, end)], ...keep.filter((i) => i >= 0 && i < count).map((i) => [i, i + 1])]
    .filter(([from, to]) => from < to)
    .sort((a, b) => a[0] - b[0]);
  const segments: Segment[] = [];
  let at = 0;
  for (const [from, to] of ranges) {
    const last = segments[segments.length - 1];
    if (last?.kind === "rows" && from <= last.end) {
      last.end = Math.max(last.end, to);
    } else {
      if (from > at) segments.push({ kind: "gap", rows: from - at });
      segments.push({ kind: "rows", start: from, end: to });
    }
    at = Math.max(at, to);
  }
  if (at < count) segments.push({ kind: "gap", rows: count - at });
  return segments;
}
