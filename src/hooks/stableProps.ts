import { useEffect, useRef } from "react";

import { changedReferences } from "../lib/stableProps";

type Check = <P extends object>(name: string, props: P, objects?: readonly (keyof P & string)[]) => void;

/**
 * In development, warns when a component that should render again only when its data changes
 * got a function, or one of `objects`, that is a new one: React Compiler memoizes by
 * dependencies and says nothing when one of them keeps changing (see "渲染与 memo" in
 * docs/ARCHITECTURE.md). Pass handlers through `useStableHandlers`. Does nothing in a release
 * build, where it is a function that calls no hooks.
 */
export const useStableProps: Check = import.meta.env.DEV
  ? function useStablePropsCheck(name, props, objects = []) {
      const previousRef = useRef<Record<string, unknown> | null>(null);
      useEffect(() => {
        const current = props as Record<string, unknown>;
        const changed = previousRef.current ? changedReferences(previousRef.current, current, objects) : [];
        if (changed.length > 0) console.warn(`${name} rendered again with new ${changed.join(", ")}; pass them through useStableHandlers`);
        previousRef.current = current;
      });
    }
  : () => {};
