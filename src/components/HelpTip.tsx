import { useState } from "react";

import { HelpIcon } from "./icons";
import { TooltipPopup } from "./Tooltip";

/** A small question mark that explains something as soon as it is hovered or focused. */
export function HelpTip({ text }: { text: string }) {
  // The icon while the tip shows: taken from the event, so that nothing reads a ref while rendering.
  const [anchor, setAnchor] = useState<HTMLSpanElement | null>(null);
  const show = (e: { currentTarget: HTMLSpanElement }) => setAnchor(e.currentTarget);
  const hide = () => setAnchor(null);
  return (
    <span
      className="help-tip"
      tabIndex={0}
      role="img"
      aria-label={text}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
      // Inside a label, a click would otherwise focus its input.
      onClick={(e) => e.preventDefault()}
    >
      <HelpIcon />
      {anchor && <TooltipPopup anchor={anchor} text={text} />}
    </span>
  );
}
