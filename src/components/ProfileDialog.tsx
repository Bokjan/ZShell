import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { deleteProfile, errorMessage, saveProfile, type AuthMethod, type Profile } from "../lib/api";

interface Props {
  /** null creates a new profile. */
  profile: Profile | null;
  /** All profiles, to pick jump hosts from. */
  profiles: Profile[];
  onClose(): void;
  onChanged(): void;
}

type AuthType = AuthMethod["type"];

export function ProfileDialog({ profile, profiles, onClose, onChanged }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState(profile?.name ?? "");
  const [host, setHost] = useState(profile?.host ?? "");
  const [port, setPort] = useState(String(profile?.port ?? 22));
  const [username, setUsername] = useState(profile?.username ?? "");
  const [authType, setAuthType] = useState<AuthType>(profile?.auth.type ?? "auto");
  const [keyPath, setKeyPath] = useState(
    profile?.auth.type === "publicKey" ? profile.auth.keyPath : "~/.ssh/id_ed25519",
  );
  const [jumpHosts, setJumpHosts] = useState<string[]>(profile?.jumpHosts ?? []);
  const [keepalive, setKeepalive] = useState(String(profile?.keepaliveInterval ?? 30));
  const [autoReconnect, setAutoReconnect] = useState(profile?.autoReconnect ?? true);
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Start expanded when the profile already uses advanced settings.
  const [advancedOpen] = useState(
    !!profile && (profile.jumpHosts.length > 0 || profile.keepaliveInterval !== 30 || !profile.autoReconnect),
  );
  // Automatic authentication falls back to a password, so it can keep a stored one too.
  const usesPassword = authType === "password" || authType === "auto";
  const profileName = (id: string) => profiles.find((p) => p.id === id)?.name ?? id;
  const jumpCandidates = profiles.filter((p) => p.id !== profile?.id && !jumpHosts.includes(p.id));

  const moveJumpHost = (index: number, offset: number) =>
    setJumpHosts((hosts) => {
      const next = [...hosts];
      [next[index], next[index + offset]] = [next[index + offset], next[index]];
      return next;
    });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const portNumber = Number(port);
    if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
      setError(t("profile.invalidPort"));
      return;
    }
    const keepaliveInterval = Number(keepalive);
    if (keepalive.trim() === "" || !Number.isInteger(keepaliveInterval) || keepaliveInterval < 0 || keepaliveInterval > 3600) {
      setError(t("profile.invalidKeepalive"));
      return;
    }
    const auth: AuthMethod =
      authType === "publicKey" ? { type: "publicKey", keyPath: keyPath.trim() } : { type: authType };
    // Only password and automatic auth keep a stored password; switching away clears it.
    let passwordUpdate: string | undefined;
    if (!usesPassword) passwordUpdate = profile ? "" : undefined;
    else if (clearPassword) passwordUpdate = "";
    else if (password) passwordUpdate = password;

    try {
      await saveProfile(
        // Forwarding rules are edited in the forwards panel; the backend keeps the saved ones.
        {
          id: profile?.id ?? "",
          name,
          host,
          port: portNumber,
          username,
          auth,
          jumpHosts,
          keepaliveInterval,
          autoReconnect,
          forwards: profile?.forwards ?? [],
        },
        passwordUpdate,
      );
      onChanged();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const remove = async () => {
    if (!profile) return;
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      return;
    }
    try {
      await deleteProfile(profile.id);
      onChanged();
      onClose();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog" onSubmit={submit}>
        <h2>{profile ? t("profile.titleEdit") : t("profile.titleNew")}</h2>

        <label>
          {t("profile.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("profile.namePlaceholder")} />
        </label>
        <div className="row">
          <label className="grow">
            {t("profile.host")}
            <input
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="example.com"
              required
              autoFocus
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
            />
          </label>
          <label className="port">
            {t("profile.port")}
            <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" required />
          </label>
        </div>
        <label>
          {t("profile.username")}
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
        </label>
        <label>
          {t("profile.auth")}
          <select value={authType} onChange={(e) => setAuthType(e.target.value as AuthType)}>
            <option value="auto">{t("profile.authAuto")}</option>
            <option value="password">{t("profile.authPassword")}</option>
            <option value="publicKey">{t("profile.authPublicKey")}</option>
            <option value="agent">{t("profile.authAgent")}</option>
          </select>
        </label>

        {authType === "auto" && <p className="hint">{t("profile.autoHint")}</p>}
        {usesPassword && (
          <>
            <label>
              {t("profile.password")}
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                disabled={clearPassword}
                placeholder={
                  profile ? t("profile.passwordKeepPlaceholder") : t("profile.passwordAskPlaceholder")
                }
              />
            </label>
            <p className="hint">
              {authType === "auto" ? t("profile.passwordAutoHint") : t("profile.passwordHint")}
            </p>
            {profile && (
              <label className="checkbox">
                <input type="checkbox" checked={clearPassword} onChange={(e) => setClearPassword(e.target.checked)} />
                {t("profile.clearPassword")}
              </label>
            )}
          </>
        )}
        {authType === "publicKey" && (
          <>
            <label>
              {t("profile.keyPath")}
              <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} required spellCheck={false} />
            </label>
            <p className="hint">{t("profile.keyHint")}</p>
          </>
        )}
        {authType === "agent" && <p className="hint">{t("profile.agentHint")}</p>}

        <details className="advanced" open={advancedOpen}>
          <summary>{t("profile.advanced")}</summary>
          <div className="field">
            <span>{t("profile.jumpHosts")}</span>
            {jumpHosts.length > 0 && (
              <ol className="jump-list">
                {jumpHosts.map((id, index) => (
                  <li key={id}>
                    <span className="jump-name">{profileName(id)}</span>
                    <button
                      type="button"
                      className="icon-button"
                      title={t("profile.moveUp")}
                      disabled={index === 0}
                      onClick={() => moveJumpHost(index, -1)}
                    >
                      ↑
                    </button>
                    <button
                      type="button"
                      className="icon-button"
                      title={t("profile.moveDown")}
                      disabled={index === jumpHosts.length - 1}
                      onClick={() => moveJumpHost(index, 1)}
                    >
                      ↓
                    </button>
                    <button
                      type="button"
                      className="icon-button"
                      title={t("profile.removeJumpHost")}
                      onClick={() => setJumpHosts((hosts) => hosts.filter((h) => h !== id))}
                    >
                      ×
                    </button>
                  </li>
                ))}
              </ol>
            )}
            <select
              value=""
              disabled={jumpCandidates.length === 0}
              onChange={(e) => e.target.value && setJumpHosts((hosts) => [...hosts, e.target.value])}
            >
              <option value="">{t("profile.addJumpHost")}</option>
              {jumpCandidates.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </div>
          <p className="hint">{t("profile.jumpHostsHint")}</p>
          <label>
            {t("profile.keepalive")}
            <input value={keepalive} onChange={(e) => setKeepalive(e.target.value)} inputMode="numeric" />
          </label>
          <p className="hint">{t("profile.keepaliveHint")}</p>
          <label className="checkbox">
            <input type="checkbox" checked={autoReconnect} onChange={(e) => setAutoReconnect(e.target.checked)} />
            {t("profile.autoReconnect")}
          </label>
        </details>

        {error && <p className="error">{error}</p>}

        <footer>
          {profile && (
            <button type="button" className="danger" onClick={remove}>
              {confirmingDelete ? t("profile.deleteConfirm") : t("common.delete")}
            </button>
          )}
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary">
            {t("common.save")}
          </button>
        </footer>
      </form>
    </div>
  );
}
