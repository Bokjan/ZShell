import { useRef, useState } from "react";

import { HelpIcon } from "./icons";
import { TooltipPopup } from "./Tooltip";

/** A small question mark that explains something as soon as it is hovered or focused. */
export function HelpTip({ text }: { text: string }) {
  const icon = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <span
      ref={icon}
      className="help-tip"
      tabIndex={0}
      role="img"
      aria-label={text}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
      // Inside a label, a click would otherwise focus its input.
      onClick={(e) => e.preventDefault()}
    >
      <HelpIcon />
      {open && icon.current && <TooltipPopup anchor={icon.current} text={text} />}
    </span>
  );
}
