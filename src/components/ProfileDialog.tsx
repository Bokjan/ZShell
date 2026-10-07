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
      setError("Port must be an integer between 1 and 65535");
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
        <h2>{profile ? "Edit Session" : "New Session"}</h2>

        <label>
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Defaults to user@host" />
        </label>
        <div className="row">
          <label className="grow">
            Host
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
            Port
            <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" required />
          </label>
        </div>
        <label>
          Username
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
          Authentication
          <select value={authType} onChange={(e) => setAuthType(e.target.value as AuthType)}>
            <option value="password">Password</option>
            <option value="publicKey">Private key file</option>
            <option value="agent">SSH Agent</option>
          </select>
        </label>

        {authType === "password" && (
          <>
            <label>
              Password
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                disabled={clearPassword}
                placeholder={profile ? "Leave empty to keep unchanged" : "Leave empty to be asked when connecting"}
              />
            </label>
            <p className="hint">Passwords are stored in the system keychain.</p>
            {profile && (
              <label className="checkbox">
                <input type="checkbox" checked={clearPassword} onChange={(e) => setClearPassword(e.target.checked)} />
                Clear saved password
              </label>
            )}
          </>
        )}
        {authType === "publicKey" && (
          <>
            <label>
              Private key path
              <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} required spellCheck={false} />
            </label>
            <p className="hint">If the key has a passphrase, you will be asked in the terminal when connecting.</p>
          </>
        )}
        {authType === "agent" && <p className="hint">Uses the keys loaded in the system SSH agent.</p>}

        {error && <p className="error">{error}</p>}

        <footer>
          {profile && (
            <button type="button" className="danger" onClick={remove}>
              {confirmingDelete ? "Click again to delete" : "Delete"}
            </button>
          )}
          <span className="grow" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="primary">
            Save
          </button>
        </footer>
      </form>
    </div>
  );
}
