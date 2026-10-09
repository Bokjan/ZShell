import { HelpIcon } from "./icons";

/** A small question mark that explains something when hovered. */
export function HelpTip({ text }: { text: string }) {
  return (
    <span className="help-tip" title={text} aria-label={text} role="img">
      <HelpIcon />
    </span>
  );
}
