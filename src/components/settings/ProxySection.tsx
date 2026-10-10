import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { proxies as proxyApi, type Proxy } from "../../lib/api";
import { ProxyDialog, proxySummary } from "../ProxyDialog";
import { Section, Setting } from "./common";

/** The saved proxies, which sessions choose on their Connection page. */
export function ProxySection() {
  const { t } = useTranslation();
  const [list, setList] = useState<Proxy[]>([]);
  // The proxy being edited; null for a new one.
  const [editing, setEditing] = useState<Proxy | null | undefined>(undefined);

  const refresh = useCallback(() => {
    proxyApi.list().then(setList).catch(console.error);
  }, []);
  useEffect(refresh, [refresh]);

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
      {editing !== undefined && (
        <ProxyDialog proxy={editing} onClose={() => setEditing(undefined)} onChanged={refresh} />
      )}
    </Section>
  );
}
