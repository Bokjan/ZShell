import { useState, type MouseEvent as ReactMouseEvent, type RefObject } from "react";

import { dragHorizontally } from "../hooks/drag";
import { useLatest } from "../hooks/latest";
import { storedString, storeString } from "../lib/storage";

const DEFAULT_WIDTH = 260;
// Room for the traffic lights and the three buttons of the title bar.
const MIN_WIDTH = 176;
const MAX_WIDTH = 480;
/** Space always left for the terminal area. */
const MIN_MAIN = 400;
const WIDTH_KEY = "zshell.sidebarWidth";

/** The width saved by the last resize; layout state, so kept per machine rather than in the settings. */
function storedWidth(): number {
  const width = Number(storedString(WIDTH_KEY));
  // One saved under other limits (the minimum used to be smaller) is brought within these.
  return width > 0 ? Math.min(Math.max(width, MIN_WIDTH), MAX_WIDTH) : DEFAULT_WIDTH;
}

const storeWidth = (width: number) => storeString(WIDTH_KEY, String(width));

const clampWidth = (width: number) => Math.round(Math.max(MIN_WIDTH, Math.min(width, MAX_WIDTH, window.innerWidth - MIN_MAIN)));

/**
 * The sidebar's width (`aside`), which its edge resizes: dragged, moved with the keyboard,
 * or back to the default; remembered once changed.
 */
export function useSidebarWidth(aside: RefObject<HTMLElement | null>) {
  const [width, setWidth] = useState(storedWidth);
  const widthRef = useLatest(width);

  const startResize = (e: ReactMouseEvent) => {
    const left = aside.current!.getBoundingClientRect().left;
    dragHorizontally(
      e,
      (x) => setWidth(clampWidth(x - left)),
      () => storeWidth(widthRef.current),
    );
  };

  const nudgeWidth = (pixels: number) => {
    const next = clampWidth(widthRef.current + pixels);
    setWidth(next);
    storeWidth(next);
  };

  const resetWidth = () => {
    setWidth(DEFAULT_WIDTH);
    storeWidth(DEFAULT_WIDTH);
  };

  /** Where the width is between the narrowest and the widest, for the edge's slider role. */
  const widthPercent = ((width - MIN_WIDTH) / (MAX_WIDTH - MIN_WIDTH)) * 100;

  return { width, widthPercent, startResize, nudgeWidth, resetWidth };
}
