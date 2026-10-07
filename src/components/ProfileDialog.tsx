import { useEffect, useState, type FormEvent } from "react";

import { deleteProfile, saveProfile, type AuthMethod, type Profile } from "../lib/api";

interface Props {
  /** null creates a new profile. */
  profile: Profile | null;
  onClose(): void;
  onChanged(): void;
}

type AuthType = AuthMethod["type"];

export function ProfileDialog({ profile, onClose, onChanged }: Props) {
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
      setError("端口必须是 1–65535 之间的整数");
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
      setError(String(err));
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
      setError(String(err));
    }
  };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog" onSubmit={submit}>
        <h2>{profile ? "编辑会话" : "新建会话"}</h2>

        <label>
          名称
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="留空则使用 user@host" />
        </label>
        <div className="row">
          <label className="grow">
            主机
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
            端口
            <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" required />
          </label>
        </div>
        <label>
          用户名
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
          认证方式
          <select value={authType} onChange={(e) => setAuthType(e.target.value as AuthType)}>
            <option value="password">密码</option>
            <option value="publicKey">私钥文件</option>
            <option value="agent">SSH Agent</option>
          </select>
        </label>

        {authType === "password" && (
          <>
            <label>
              密码
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                disabled={clearPassword}
                placeholder={profile ? "留空则保持不变" : "留空则在连接时询问"}
              />
            </label>
            <p className="hint">密码保存在系统钥匙串中。</p>
            {profile && (
              <label className="checkbox">
                <input type="checkbox" checked={clearPassword} onChange={(e) => setClearPassword(e.target.checked)} />
                清除已保存的密码
              </label>
            )}
          </>
        )}
        {authType === "publicKey" && (
          <>
            <label>
              私钥路径
              <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} required spellCheck={false} />
            </label>
            <p className="hint">如果私钥有密码短语，连接时会在终端中询问。</p>
          </>
        )}
        {authType === "agent" && <p className="hint">使用系统 SSH agent 中已加载的密钥。</p>}

        {error && <p className="error">{error}</p>}

        <footer>
          {profile && (
            <button type="button" className="danger" onClick={remove}>
              {confirmingDelete ? "再次点击确认删除" : "删除"}
            </button>
          )}
          <span className="grow" />
          <button type="button" onClick={onClose}>
            取消
          </button>
          <button type="submit" className="primary">
            保存
          </button>
        </footer>
      </form>
    </div>
  );
}
