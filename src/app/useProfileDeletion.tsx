import { useState } from "react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { deleteProfile, forwards, type Profile, type SessionId } from "../ipc/api";
import { directory } from "../lib/directory";
import { paneLabel } from "../lib/panes";
import { currentSettings } from "../lib/settings";
import type { TabStore } from "../lib/tabs";
import { useLocale } from "../hooks/locale";

interface Asking {
  profile: Profile;
  /** The tabs its port forwarding runs in, as a list in a sentence; null if it runs in none. */
  tabs: string | null;
  answer(confirmed: boolean): void;
}

/**
 * Deleting a saved session. Asked first, unless the user has confirmed already (the session's
 * dialog); also then when its port forwarding runs in tabs, which deleting stops: those tabs
 * are named (those of other windows counted), and the user deletes anyway or keeps it.
 * `dialog` is what to render for the question.
 */
export function useProfileDeletion(store: TabStore) {
  const { t } = useTranslation();
  const { formatList } = useLocale();
  const [asking, setAsking] = useState<Asking | null>(null);

  /** The tabs running `profile`'s forwarding (see `Asking`). */
  const forwardingTabs = async (profile: Profile): Promise<string | null> => {
    const ids = await forwards.sessionsOf(profile.id).catch((): SessionId[] => []);
    if (ids.length === 0) return null;
    const { followRemoteTitle } = currentSettings().tabs;
    const here = store
      .get()
      .tabs.flatMap((tab) =>
        tab.panes
          .filter((pane) => pane.sessionId != null && ids.includes(pane.sessionId))
          .map((pane) => paneLabel(tab, pane, followRemoteTitle)),
      );
    const elsewhere = ids.length - here.length;
    const names = here.map((name) => t("sidebar.forwardingTab", { name }));
    return formatList(elsewhere > 0 ? [...names, t("sidebar.otherWindowTabs", { count: elsewhere })] : names);
  };

  /**
   * Deletes `profile` once the user confirms, or at once if `confirmed` already and no
   * forwarding of it runs; resolves to whether it was deleted, rejects if that failed.
   */
  const remove = async (profile: Profile, confirmed: boolean): Promise<boolean> => {
    const tabs = await forwardingTabs(profile);
    const go = confirmed && tabs === null ? true : await new Promise<boolean>((answer) => setAsking({ profile, tabs, answer }));
    if (!go) return false;
    await directory.changed(deleteProfile(profile.id));
    return true;
  };

  const answer = (confirmed: boolean) => {
    asking?.answer(confirmed);
    setAsking(null);
  };

  const dialog = asking && (
    <ConfirmDialog
      title={t("sidebar.deleteTitle")}
      message={
        asking.tabs === null
          ? t("sidebar.deleteMessage", { name: asking.profile.name })
          : t("sidebar.deleteForwardsMessage", { name: asking.profile.name, tabs: asking.tabs })
      }
      confirmLabel={asking.tabs === null ? t("common.delete") : t("sidebar.deleteAnyway")}
      danger
      onConfirm={() => answer(true)}
      onCancel={() => answer(false)}
    />
  );

  return { remove, dialog };
}
