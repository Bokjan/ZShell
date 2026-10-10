import { useRef, useState } from "react";

/**
 * `handlers` as one object that stays the same from render to render, whose functions call
 * the latest render's: for a memoized component that should render again only when its other
 * props change. Values that aren't functions are those of the first render.
 */
export function useStableHandlers<T extends object>(handlers: T): T {
  const latest = useRef(handlers);
  latest.current = handlers;
  const [stable] = useState(() => {
    const entries = Object.entries(handlers).map(([name, value]) => [
      name,
      typeof value === "function" ? (...args: unknown[]) => (latest.current as Record<string, (...a: unknown[]) => unknown>)[name](...args) : value,
    ]);
    return Object.fromEntries(entries) as T;
  });
  return stable;
}
