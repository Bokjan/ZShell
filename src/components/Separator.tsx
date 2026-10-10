import type { CSSProperties, KeyboardEvent, MouseEvent } from "react";

/** How far one press of an arrow key moves a separator, in pixels; with Shift, `BIG_STEP`. */
const STEP = 16;
const BIG_STEP = 64;

interface Props {
  /** Its name for screen readers: what it resizes. */
  label: string;
  /** "vertical" between things side by side (it moves left and right), "horizontal" between stacked ones. */
  orientation: "vertical" | "horizontal";
  /** Where it is, as a percentage of the room it moves in. */
  value: number;
  /** Moves it by `pixels` from the keyboard: negative is left or up. The caller keeps it within its limits. */
  onMove(pixels: number): void;
  /** Enter and a double-click: equal sizes, or the default size. */
  onReset?(): void;
  onMouseDown(e: MouseEvent): void;
  className: string;
  style?: CSSProperties;
  title?: string;
}

/**
 * An edge that resizes what is on either side of it: dragged with the mouse, or focused and
 * moved with the arrow keys (a window splitter, in ARIA terms).
 */
export function Separator({ label, orientation, value, onMove, onReset, onMouseDown, className, style, title }: Props) {
  const onKeyDown = (e: KeyboardEvent) => {
    const step = e.shiftKey ? BIG_STEP : STEP;
    const [back, forward] = orientation === "vertical" ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"];
    if (e.key === back) onMove(-step);
    else if (e.key === forward) onMove(step);
    else if (e.key === "Enter" && onReset) onReset();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div
      className={className}
      style={style}
      title={title}
      role="separator"
      tabIndex={0}
      aria-label={label}
      aria-orientation={orientation}
      aria-valuenow={Math.round(value)}
      aria-valuemin={0}
      aria-valuemax={100}
      onKeyDown={onKeyDown}
      onMouseDown={onMouseDown}
      onDoubleClick={onReset}
    />
  );
}
