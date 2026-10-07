import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { deleteProfile, errorMessage, saveProfile, type AuthMethod, type Profile } from "../lib/api";

interface Props {
  /** null creates a new profile. */
  profile: Profile | null;
  onClose(): void;
  onChanged(): void;
}

type AuthType = AuthMethod["type"];

export function ProfileDialog({ profile, onClose, onChanged }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState(profile?.name ?? "");
  const [host, setHost] = useState(profile?.host ?? "");
  const [port, setPort] = useState(String(profile?.port ?? 22));
  const [username, setUsername] = useState(profile?.username ?? "");
  const [authType, setAuthType] = useState<AuthType>(profile?.auth.type ?? "password");
  const [keyPath, setKeyPath] = useState(
    profile?.auth.type === "publicKey" ? profile.auth.keyPath : "~/.ssh/id_ed25519",
  );
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
    const auth: AuthMethod =
      authType === "publicKey" ? { type: "publicKey", keyPath: keyPath.trim() } : { type: authType };
    // Only password auth keeps a stored password; switching away clears it.
    let passwordUpdate: string | undefined;
    if (authType !== "password") passwordUpdate = profile ? "" : undefined;
    else if (clearPassword) passwordUpdate = "";
    else if (password) passwordUpdate = password;

    try {
      await saveProfile(
        { id: profile?.id ?? "", name, host, port: portNumber, username, auth },
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
            <option value="password">{t("profile.authPassword")}</option>
            <option value="publicKey">{t("profile.authPublicKey")}</option>
            <option value="agent">{t("profile.authAgent")}</option>
          </select>
        </label>

        {authType === "password" && (
          <>
            <label>
              {t("profile.password")}
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                disabled={clearPassword}
                placeholder={profile ? t("profile.passwordKeepPlaceholder") : t("profile.passwordAskPlaceholder")}
              />
            </label>
            <p className="hint">{t("profile.passwordHint")}</p>
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
