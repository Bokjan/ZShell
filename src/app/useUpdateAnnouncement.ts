import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { announce } from "../lib/announce";

/** What the update check found (see `useUpdates`). */
interface Updates {
  loaded: boolean;
  available: { version: string } | null;
}

/**
 * Announces a newer version found while the app runs (not one known at the start, marked
 * already) in the window in use, unless About is open (`aboutOpen`), which tells of the checks
 * made there.
 */
export function useUpdateAnnouncement(updates: Updates, aboutOpen: boolean) {
  const { t } = useTranslation();
  const announced = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    if (!updates.loaded) return;
    const version = updates.available?.version ?? null;
    const known = announced.current;
    announced.current = version;
    if (known !== undefined && version && version !== known && document.hasFocus() && !aboutOpen) {
      announce(t("updates.found", { version }));
    }
  }, [updates.loaded, updates.available?.version, aboutOpen, t]);
}
