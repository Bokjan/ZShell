import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import type { useUpdates } from "../hooks/updates";
import { announce } from "../lib/announce";

/**
 * Announces a newer version found while the app runs (not one known at the start, marked
 * already) in the window in use, unless settings are open (`settingsOpen`), whose About
 * section tells of the checks made there.
 */
export function useUpdateAnnouncement(updates: ReturnType<typeof useUpdates>, settingsOpen: boolean) {
  const { t } = useTranslation();
  const announced = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    if (!updates.loaded) return;
    const version = updates.available?.version ?? null;
    const known = announced.current;
    announced.current = version;
    if (known !== undefined && version && version !== known && document.hasFocus() && !settingsOpen) {
      announce(t("updates.found", { version }));
    }
  }, [updates.loaded, updates.available?.version, settingsOpen, t]);
}
