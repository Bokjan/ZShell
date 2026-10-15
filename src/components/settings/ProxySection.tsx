import { useState } from "react";
import { useTranslation } from "react-i18next";

import type { Proxy } from "../../ipc/api";
import { useDirectory } from "../../hooks/stores";
import { ProxyDialog, proxySummary } from "../ProxyDialog";
import { Section, Setting } from "./common";

/** The saved proxies, which sessions choose on their Connection page. */
export function ProxySection() {
  const { t } = useTranslation();
  const { proxies: list } = useDirectory();
  // The proxy being edited; null for a new one.
  const [editing, setEditing] = useState<Proxy | null | undefined>(undefined);

  return (
    <Section id="proxies">
      <Setting>
        {list.length > 0 && (
          <ul className="proxy-list">
            {list.map((proxy) => (
              <li key={proxy.id}>
                <button type="button" onClick={() => setEditing(proxy)}>
                  <span className="proxy-name">{proxy.name}</span>
                  <span className="proxy-summary">{proxySummary(t, proxy)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="row">
          <button type="button" onClick={() => setEditing(null)}>
            {t("settings.addProxy")}
          </button>
        </div>
        <p className="hint">{t("settings.proxiesHint")}</p>
      </Setting>
      {editing !== undefined && <ProxyDialog proxy={editing} onClose={() => setEditing(undefined)} />}
    </Section>
  );
}
