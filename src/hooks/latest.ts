import { useInsertionEffect, useRef, type RefObject } from "react";

/**
 * A ref holding the latest `value`, for event handlers and callbacks handed elsewhere that must
 * see the current one without changing themselves. Name it `…Ref`: React Compiler treats only
 * those as refs, and skips a component that writes to anything else it got from a hook.
 *
 * The ref is set after rendering rather than during it (Compiler skips components that write
 * refs while rendering), in an insertion effect: those run before every layout effect, so a
 * child's layout effect calling back into its parent already sees the parent's new values.
 * Callbacks that only run inside effects take `useEffectEvent` instead.
 */
export function useLatest<T>(value: T): RefObject<T> {
  const ref = useRef(value);
  useInsertionEffect(() => {
    ref.current = value;
  });
  return ref;
}
