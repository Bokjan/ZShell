import { useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";

import type { PatchStore } from "../../lib/store";

/** The size shown, and how many times it has been shown, which restarts its fading. */
export interface SizeShown {
  size: { cols: number; rows: number; count: number } | null;
}

/**
 * The terminal's size as it is resized. A component of its own, so that showing it as the size
 * changes each frame doesn't render the whole terminal view.
 */
export function SizeOverlay({ shown, fontFamily }: { shown: PatchStore<SizeShown>; fontFamily: string }) {
  const { t } = useTranslation();
  const { size } = useSyncExternalStore(shown.subscribe, shown.get);
  if (!size) return null;
  return (
    // The status bar has the size for screen readers.
    <div key={size.count} className="size-overlay" aria-hidden="true" style={{ fontFamily }}>
      {t("terminal.size", { cols: size.cols, rows: size.rows })}
    </div>
  );
}
