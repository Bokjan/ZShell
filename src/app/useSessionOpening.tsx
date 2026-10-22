import { useState } from "react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../components/ConfirmDialog";
import type { OpenMode } from "../components/Sidebar";
import type { Folder, Profile, SessionTarget } from "../ipc/api";
import { directory } from "../lib/directory";
import { address, loginName, sessionsIn, type QuickTarget } from "../lib/sessions";
import type { SessionRegistry } from "../lib/sessionRegistry";
import { activeTabOf, type TabStore } from "../lib/tabs";
import type { usePaneActions } from "./usePaneActions";
import type { useWindows } from "./useWindows";

interface Options {
  paneActions: ReturnType<typeof usePaneActions>;
  windowing: ReturnType<typeof useWindows>;
  /** The local shell's name ("zsh"), which titles local terminals, once known. */
  shellName: string | null;
  /** The local user's name, for quick SSH connections without one. */
  username: string;
  /** Moves the keyboard focus back to the active terminal. */
  focusTerminal(): void;
}

/**
 * Opening sessions: saved ones from the session list (all of a folder's, asking first when
 * they are several), quick connections and local terminals. `dialog` is what to render for
 * the question.
 */
export function useSessionOpening(store: TabStore, sessions: SessionRegistry, options: Options) {
  const { t } = useTranslation();
  const { paneActions, windowing } = options;
  // A folder's sessions waiting for confirmation to open them all.
  const [openingAll, setOpeningAll] = useState<{ folder: Folder; profiles: Profile[] } | null>(null);

  /**
   * "connect" connects again in the focused pane of the active tab when that is the session's
   * and its connection is closed (or waiting to reconnect), else opens a tab; "newTab" always
   * opens a tab; "splitRight" and "splitDown" split the focused pane (or open a tab if there
   * is none); "newWindow" opens a window with a tab for it.
   */
  const openProfile = (profile: Profile, mode: OpenMode) => {
    directory.opened(profile.id);
    if (mode === "newWindow") {
      windowing.openInNewWindow(profile.id, profile.name);
      return;
    }
    const target: SessionTarget = { kind: "profile", profileId: profile.id };
    const active = activeTabOf(store.get());
    const here = active?.panes.find((p) => p.key === active.focused);
    if (mode === "connect" && here?.status === "closed" && here.target.kind === "profile" && here.target.profileId === profile.id) {
      sessions.reconnect(here.key);
      options.focusTerminal();
    } else if ((mode === "splitRight" || mode === "splitDown") && active) {
      paneActions.splitPane(active.focused, mode === "splitRight" ? "row" : "column", paneActions.specOf(target, profile.name));
    } else paneActions.openTab(target, profile.name);
  };

  const openAll = (folder: Folder) => {
    const list = sessionsIn(folder.id, directory.get());
    // Several tabs (and connections) at once ask first; a single session opens as it would by itself.
    if (list.length > 1) setOpeningAll({ folder, profiles: list });
    else list.forEach((p) => openProfile(p, "newTab"));
  };

  const quickConnect = (target: QuickTarget) =>
    paneActions.openTab({ kind: "quick", ...target }, address({ ...target, username: loginName(target, options.username) }));

  const openLocal = () => paneActions.openTab({ kind: "local" }, options.shellName ?? t("tabs.localTitle"));

  const dialog = openingAll && (
    <ConfirmDialog
      title={t("sidebar.openAllTitle", { count: openingAll.profiles.length })}
      message={t("sidebar.openAllMessage", { count: openingAll.profiles.length, folder: openingAll.folder.name })}
      confirmLabel={t("sidebar.openAllConfirm")}
      onConfirm={() => {
        openingAll.profiles.forEach((p) => openProfile(p, "newTab"));
        setOpeningAll(null);
      }}
      onCancel={() => setOpeningAll(null)}
    />
  );

  return { openProfile, openAll, quickConnect, openLocal, dialog };
}
