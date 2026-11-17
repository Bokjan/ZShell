import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { confirmLocalProfile, errorMessage } from "../ipc/api";
import { useDirectory } from "../hooks/stores";
import { announce } from "../lib/announce";
import { directory } from "../lib/directory";
import { formatArgs, localOptions } from "../lib/localShells";
import type { SessionRegistry } from "../lib/sessionRegistry";
import type { TabStore } from "../lib/tabs";

/**
 * Asks before an imported local session first runs: it runs a program here and types commands
 * into it, as the file says, and the file may be someone else's. Asked when a pane's session
 * didn't open for want of it (`local.notConfirmed`), from wherever it was opened; confirming
 * lets it run from then on, and opens it again in the panes of this window waiting for it.
 * `dialog` is what to render for the question.
 */
export function useLocalConfirmation(store: TabStore, sessions: SessionRegistry) {
  const { t } = useTranslation();
  const { profiles } = useDirectory();
  // The session asked about, by id.
  const [asking, setAsking] = useState<string | null>(null);
  useEffect(() => sessions.on("unconfirmed", (_key, profileId) => setAsking((current) => current ?? profileId)), [sessions]);

  const profile = asking === null ? undefined : profiles.find((p) => p.id === asking);
  const local = localOptions(profile);

  const confirm = async (id: string) => {
    setAsking(null);
    try {
      await directory.changed(confirmLocalProfile(id));
    } catch (e) {
      announce(errorMessage(e));
      console.error(e);
      return;
    }
    for (const pane of store.get().tabs.flatMap((tab) => tab.panes)) {
      if (pane.target.kind === "profile" && pane.target.profileId === id && pane.status === "closed") sessions.reconnect(pane.key);
    }
  };

  // Gone, or confirmed in another window meanwhile: nothing to ask (Enter in the pane opens it).
  const dialog = profile && local?.imported && (
    <ConfirmDialog
      title={t("localConfirm.title")}
      message={t("localConfirm.message", { name: profile.name })}
      confirmLabel={t("localConfirm.open")}
      onConfirm={() => void confirm(profile.id)}
      onCancel={() => setAsking(null)}
    >
      <dl className="local-review">
        {local.shell.kind === "program" && (
          <>
            <dt>{t("profile.program")}</dt>
            <dd>{local.shell.path}</dd>
            {local.shell.args.length > 0 && (
              <>
                <dt>{t("profile.args")}</dt>
                <dd>{formatArgs(local.shell.args)}</dd>
              </>
            )}
          </>
        )}
        {local.shell.kind === "wsl" && (
          <>
            <dt>{t("profile.distro")}</dt>
            <dd>{local.shell.distro}</dd>
          </>
        )}
        {local.shell.kind === "default" && (
          <>
            <dt>{t("profile.shell")}</dt>
            <dd>{t("profile.defaultShell")}</dd>
          </>
        )}
        {local.startDir && (
          <>
            <dt>{t("profile.startDir")}</dt>
            <dd>{local.startDir}</dd>
          </>
        )}
        {local.env.length > 0 && (
          <>
            <dt>{t("profile.env")}</dt>
            <dd>{local.env.map((v) => `${v.name}=${v.value}`).join("\n")}</dd>
          </>
        )}
        {profile.loginCommands.length > 0 && (
          <>
            <dt>{t("profile.loginCommands")}</dt>
            <dd>{profile.loginCommands.join("\n")}</dd>
          </>
        )}
      </dl>
    </ConfirmDialog>
  );
  return { dialog };
}
