/**
 * Terminals that draw with WebGL at once, at most. The web view allows about 16 contexts and
 * drops the oldest beyond that, which makes its terminal fall back to the DOM renderer and try
 * again later, dropping another: the rest are left for the terminals of a tab just hidden, whose
 * contexts are released only after the shown tab's terminals have made theirs.
 */
export const WEBGL_BUDGET = 12;

/**
 * Which of the terminals drawing with WebGL (`used` saying when each was last loaded or focused)
 * give up their context so that `keep` are left: the least recently used.
 */
export function beyondBudget<T extends { used: number }>(drawing: readonly T[], keep: number): T[] {
  return [...drawing].sort((a, b) => b.used - a.used).slice(Math.max(keep, 0));
}
