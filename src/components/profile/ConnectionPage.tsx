import { useTranslation } from "react-i18next";

import type { Profile, Proxy } from "../../lib/api";
import type { ProfileForm } from "../../lib/profileForm";
import { IconButton } from "../IconButton";
import { ArrowIcon, CloseIcon } from "../icons";

/** The proxy choice that opens a dialog to create one. */
const NEW_PROXY = "\u0000new";

/** The proxy of an SSH or Telnet session. */
const proxyOf = (profile: Profile) => (profile.connection.protocol === "serial" ? undefined : profile.connection.proxy);

interface Props {
  form: ProfileForm;
  set(patch: Partial<ProfileForm>): void;
  /** The profile edited; null for a new one. */
  profile: Profile | null;
  /** All profiles, to pick jump hosts from. */
  profiles: Profile[];
  /** The proxies; null until loaded. */
  proxies: Proxy[] | null;
  newProxy(): void;
}

/** The session dialog's page of how it connects: jump hosts, proxy, keepalives, reconnecting. */
export function ConnectionPage({ form, set, profile, profiles, proxies, newProxy }: Props) {
  const { t } = useTranslation();
  const { protocol, jumpHosts } = form;
  const ssh = protocol === "ssh";
  const profileName = (id: string) => profiles.find((p) => p.id === id)?.name ?? id;
  // Only SSH sessions can be jump hosts.
  const candidates = profiles.filter((p) => p.id !== profile?.id && p.connection.protocol === "ssh" && !jumpHosts.includes(p.id));
  // A proxy that is missing (its file was set aside) is shown as such: the session doesn't
  // connect until it is restored or another is chosen.
  const isMissing = (id: string | undefined) => !!id && !!proxies && !proxies.some((p) => p.id === id);
  const proxyLabel = (id: string | undefined) =>
    !id ? t("profile.noProxy") : (proxies?.find((p) => p.id === id)?.name ?? (proxies ? t("profile.missingProxy") : ""));
  // With jump hosts, the first one's own proxy is used.
  const firstJump = profiles.find((p) => p.id === jumpHosts[0]);
  const shownProxy = firstJump ? (proxyOf(firstJump) ?? "") : form.proxy;

  const moveJumpHost = (index: number, offset: number) => {
    const next = [...jumpHosts];
    [next[index], next[index + offset]] = [next[index + offset], next[index]];
    set({ jumpHosts: next });
  };

  return (
    <>
      {protocol !== "serial" && (
        <>
          <div className="field">
            <span>{t("profile.jumpHosts")}</span>
            {jumpHosts.length > 0 && (
              <ol className="jump-list">
                {jumpHosts.map((id, index) => (
                  <li key={id}>
                    <span className="jump-name">{profileName(id)}</span>
                    <IconButton
                      type="button"
                      className="icon-button"
                      label={t("profile.moveUp", { name: profileName(id) })}
                      disabled={index === 0}
                      onClick={() => moveJumpHost(index, -1)}
                    >
                      <ArrowIcon direction="up" size={12} />
                    </IconButton>
                    <IconButton
                      type="button"
                      className="icon-button"
                      label={t("profile.moveDown", { name: profileName(id) })}
                      disabled={index === jumpHosts.length - 1}
                      onClick={() => moveJumpHost(index, 1)}
                    >
                      <ArrowIcon direction="down" size={12} />
                    </IconButton>
                    <IconButton
                      type="button"
                      className="icon-button"
                      label={t("profile.removeJumpHost", { name: profileName(id) })}
                      onClick={() => set({ jumpHosts: jumpHosts.filter((h) => h !== id) })}
                    >
                      <CloseIcon size={12} />
                    </IconButton>
                  </li>
                ))}
              </ol>
            )}
            <select
              value=""
              disabled={candidates.length === 0}
              onChange={(e) => e.target.value && set({ jumpHosts: [...jumpHosts, e.target.value] })}
            >
              <option value="">{t("profile.addJumpHost")}</option>
              {candidates.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
          <p className="hint">{t(ssh ? "profile.jumpHostsHint" : "profile.jumpHostsHintTelnet")}</p>
          <label>
            {t("profile.proxy")}
            <select
              value={shownProxy}
              disabled={!!firstJump}
              onChange={(e) => (e.target.value === NEW_PROXY ? newProxy() : set({ proxy: e.target.value }))}
            >
              <option value="">{t("profile.noProxy")}</option>
              {isMissing(shownProxy) && <option value={shownProxy}>{t("profile.missingProxy")}</option>}
              {proxies?.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
              <option value={NEW_PROXY}>{t("profile.newProxy")}</option>
            </select>
          </label>
          <p className="hint">
            {firstJump
              ? t("profile.proxyViaJumpHost", { name: firstJump.name, proxy: proxyLabel(proxyOf(firstJump)) })
              : t(isMissing(form.proxy) ? "profile.missingProxyHint" : "profile.proxyHint")}
          </p>
          <label>
            {t("profile.keepalive")}
            <input name="keepalive" value={form.keepalive} onChange={(e) => set({ keepalive: e.target.value })} inputMode="numeric" />
          </label>
          <p className="hint">{t(ssh ? "profile.keepaliveHint" : "profile.keepaliveHintTelnet")}</p>
        </>
      )}
      <label className="checkbox">
        <input type="checkbox" checked={form.autoReconnect} onChange={(e) => set({ autoReconnect: e.target.checked })} />
        {t(protocol === "serial" ? "profile.autoReconnectSerial" : "profile.autoReconnect")}
      </label>
      {ssh && (
        <>
          <label className="checkbox">
            <input type="checkbox" checked={form.forwardAgent} onChange={(e) => set({ forwardAgent: e.target.checked })} />
            {t("profile.forwardAgent")}
          </label>
          <p className="hint">{t("profile.forwardAgentHint")}</p>
        </>
      )}
    </>
  );
}
