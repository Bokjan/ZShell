import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";

import { announce } from "../../lib/announce";
import type { ReadBack } from "../../lib/readBack";
import { ArrowIcon } from "../icons";

/** How often the count is drawn again while output keeps coming: often enough to follow, without a render every frame. */
const COUNT_MS = 250;
/** The largest count shown as it is; past it the button says "999+", and keeps its width. */
const MOST_SHOWN = 999;

interface Props {
  readBack: ReadBack;
  /** Whether the terminal is shown; out of sight the count is left alone until it is. */
  visible: boolean;
  onClick(): void;
}

/**
 * The lines of output that arrived below the lines being read back (see `lib/readBack`), as
 * a button back to the end. A component of its own, so that the count drawn again doesn't
 * render the whole terminal view. It shows and goes at once, and the count changes at most
 * every `COUNT_MS`, in a place of its own width (`.new-output-count`) so that the button
 * doesn't move as it does. Screen readers are told once each time it shows.
 */
export function NewOutputButton({ readBack, visible, onClick }: Props) {
  const { t } = useTranslation();
  const [lines, setLines] = useState(readBack.newLines);

  useEffect(() => {
    if (!visible) return;
    let shown = readBack.newLines;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const update = () => {
      timer = undefined;
      const now = readBack.newLines;
      if (now > 0 && shown === 0) announce(t("terminal.newOutput"));
      shown = now;
      setLines(now);
    };
    update();
    const unsubscribe = readBack.subscribe(() => {
      if (readBack.newLines === 0 || shown === 0) {
        clearTimeout(timer);
        update();
      } else if (timer === undefined) timer = setTimeout(update, COUNT_MS);
    });
    return () => {
      unsubscribe();
      clearTimeout(timer);
    };
  }, [readBack, visible, t]);

  if (lines === 0) return null;
  return (
    // Not focused by a click, which leaves the keyboard with the terminal.
    <button className="new-output" title={t("terminal.scrollToEnd")} onMouseDown={(e) => e.preventDefault()} onClick={onClick}>
      <ArrowIcon direction="down" size={12} />
      <span>
        <Trans
          i18nKey="terminal.newLines"
          count={lines}
          values={{ lines: lines > MOST_SHOWN ? `${MOST_SHOWN}+` : String(lines) }}
          components={{ count: <span className="new-output-count" /> }}
        />
      </span>
    </button>
  );
}
