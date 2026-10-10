import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { knownHosts } from "../../lib/api";
import { isMac } from "../../lib/platform";
import { HelpTip } from "../HelpTip";
import { KnownHostsDialog } from "../KnownHostsDialog";
import { Section, Setting } from "./common";

/** The host keys in `~/.ssh/known_hosts`, managed in their own dialog. */
export function KnownHostsSection() {
  const { t } = useTranslation();
  const [path, setPath] = useState<string | null>(null);
  const [count, setCount] = useState<number | null>(null);
  const [managing, setManaging] = useState(false);

  const refresh = useCallback(() => {
    knownHosts.path().then(setPath).catch(console.error);
    knownHosts.list().then((entries) => setCount(entries.length), console.error);
  }, []);
  useEffect(refresh, [refresh]);

  return (
    <Section id="knownHosts">
      <Setting>
        {count !== null && (
          <p className="known-hosts-summary">
            {t("settings.knownHostsCount", { count, file: "~/.ssh/known_hosts" })}
            {path && <HelpTip text={path} />}
          </p>
        )}
        <div className="row">
          <button type="button" onClick={() => setManaging(true)}>
            {t("settings.knownHostsManage")}
          </button>
          <button
            type="button"
            disabled={!path || !count}
            onClick={() => path && void revealItemInDir(path).catch(console.error)}
          >
            {t(isMac ? "settings.knownHostsReveal" : "settings.knownHostsRevealWindows")}
          </button>
        </div>
        <p className="hint">{t("settings.knownHostsHint")}</p>
      </Setting>
      {managing && (
        <KnownHostsDialog
          onClose={() => {
            setManaging(false);
            refresh();
          }}
          onChanged={refresh}
        />
      )}
    </Section>
  );
}
