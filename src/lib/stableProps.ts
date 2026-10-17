/**
 * The props of a memoized component that are new from one render to the next although they
 * shouldn't be: those holding a function, and `objects` (handlers objects and the like), by
 * reference. Values that can change from render to render (a tab, a count) are left out.
 */
export function changedReferences(
  before: Record<string, unknown>,
  after: Record<string, unknown>,
  objects: readonly string[] = [],
): string[] {
  const names = new Set([...Object.keys(before), ...Object.keys(after)]);
  return [...names].filter((name) => {
    const checked = typeof before[name] === "function" || typeof after[name] === "function" || objects.includes(name);
    return checked && before[name] !== after[name];
  });
}
