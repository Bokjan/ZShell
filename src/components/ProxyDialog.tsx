import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";

import { errorMessage, proxies, type Proxy, type ProxyKind } from "../lib/api";
import { hostPort, wholeNumber } from "../lib/format";
import { isComposing } from "../lib/platform";

interface Props {
  /** null creates a new proxy. */
  proxy: Proxy | null;
  onClose(): void;
  /** Called with the saved proxy. */
  onSaved?(proxy: Proxy): void;
  /** Called after saving or deleting, also when only storing the password failed. */
  onChanged(): void;
}

const KINDS: ProxyKind[] = ["socks5", "http", "command"];

/** The usual port of each kind of proxy server. */
const DEFAULT_PORTS = { socks5: 1080, http: 8080 } as const;

/** One line saying what a proxy is, e.g. "SOCKS5 127.0.0.1:7890" or the command. */
export function proxySummary(t: TFunction, proxy: Proxy) {
  if (proxy.kind === "command") return proxy.command;
  const user = proxy.username ? `${proxy.username}@` : "";
  return `${t(`proxy.kinds.${proxy.kind}`)} ${user}${hostPort(proxy.host, proxy.port)}`;
}

/** Creates, edits or deletes a proxy. */
export function ProxyDialog({ proxy: initial, onClose, onSaved, onChanged }: Props) {
  const { t } = useTranslation();
  // A new proxy becomes the saved one when only its password failed (see ProfileDialog).
  const [proxy, setProxy] = useState(initial);
  const [name, setName] = useState(proxy?.name ?? "");
  const [kind, setKind] = useState<ProxyKind>(proxy?.kind ?? "socks5");
  const [host, setHost] = useState(proxy?.host ?? "");
  const [port, setPort] = useState(String(proxy?.port || DEFAULT_PORTS.socks5));
  const [username, setUsername] = useState(proxy?.username ?? "");
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [command, setCommand] = useState(proxy?.command ?? "");
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const server = kind !== "command";

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || isComposing(e)) return;
      // Only this dialog closes, not the one it was opened from.
      e.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  // The port follows the kind unless it was changed from a usual one.
  const changeKind = (next: ProxyKind) => {
    const usual = Object.values(DEFAULT_PORTS).map(String);
    if (next !== "command" && usual.includes(port)) setPort(String(DEFAULT_PORTS[next]));
    setKind(next);
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const portNumber = wholeNumber(port);
    if (server && !host.trim()) {
      setError(t("proxy.missingHost"));
      return;
    }
    if (server && (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535)) {
      setError(t("profile.invalidPort"));
      return;
    }
    if (!server && !command.trim()) {
      setError(t("proxy.missingCommand"));
      return;
    }
    // Only a SOCKS or HTTP proxy with a user name keeps a password; the backend drops it otherwise.
    let passwordUpdate: string | undefined;
    if (!server || !username.trim()) passwordUpdate = undefined;
    else if (clearPassword) passwordUpdate = "";
    else if (password) passwordUpdate = password;
    setSaving(true);
    try {
      const { saved, passwordError } = await proxies.save(
        {
          id: proxy?.id ?? "",
          name,
          kind,
          host,
          port: Number.isInteger(portNumber) && portNumber > 0 && portNumber <= 65535 ? portNumber : (proxy?.port ?? 0),
          username,
          command,
        },
        passwordUpdate,
      );
      onChanged();
      onSaved?.(saved);
      if (passwordError) {
        setProxy(saved);
        setError(t("proxy.passwordNotSaved", { message: passwordError.message }));
        setSaving(false);
        return;
      }
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
    }
  };

  const remove = async () => {
    if (!proxy) return;
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      return;
    }
    try {
      await proxies.delete(proxy.id);
      onChanged();
      onClose();
    } catch (err) {
      setConfirmingDelete(false);
      setError(errorMessage(err));
    }
  };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog proxy-dialog" onSubmit={submit}>
        <h2>{proxy ? t("proxy.titleEdit") : t("proxy.titleNew")}</h2>

        <div className="row">
          <label className="grow">
            {t("proxy.name")}
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t(server ? "proxy.namePlaceholder" : "proxy.namePlaceholderCommand")}
            />
          </label>
          <label>
            {t("proxy.kind")}
            <select value={kind} onChange={(e) => changeKind(e.target.value as ProxyKind)}>
              {KINDS.map((value) => (
                <option key={value} value={value}>
                  {t(`proxy.kinds.${value}`)}
                </option>
              ))}
            </select>
          </label>
        </div>

        {server ? (
          <>
            <div className="row">
              <label className="grow">
                {t("profile.host")}
                <input
                  value={host}
                  onChange={(e) => setHost(e.target.value)}
                  placeholder="127.0.0.1"
                  autoFocus
                  autoCapitalize="off"
                  autoCorrect="off"
                  spellCheck={false}
                />
              </label>
              <label className="port">
                {t("profile.port")}
                <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" />
              </label>
            </div>
            <label>
              {t("profile.username")}
              <input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder={t("proxy.usernamePlaceholder")}
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
              />
            </label>
            {username.trim() && (
              <>
                <label>
                  {t("profile.password")}
                  <input
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    disabled={clearPassword}
                    placeholder={proxy?.username ? t("profile.passwordKeepPlaceholder") : t("profile.passwordAskPlaceholder")}
                  />
                </label>
                {proxy?.username && (
                  <label className="checkbox">
                    <input type="checkbox" checked={clearPassword} onChange={(e) => setClearPassword(e.target.checked)} />
                    {t("profile.clearPassword")}
                  </label>
                )}
              </>
            )}
            <p className="hint">{t(kind === "socks5" ? "proxy.socksHint" : "proxy.httpHint")}</p>
          </>
        ) : (
          <>
            <label>
              {t("proxy.command")}
              <textarea
                className="command-text"
                rows={2}
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder="nc -X connect -x proxy.example.com:8080 %h %p"
                autoFocus
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
              />
            </label>
            <p className="hint">{t("proxy.commandHint")}</p>
            <p className="hint">{t("proxy.commandPasswordHint")}</p>
          </>
        )}

        {error && <p className="error">{error}</p>}

        <footer>
          {proxy && (
            <button type="button" className="danger" onClick={() => void remove()} onBlur={() => setConfirmingDelete(false)}>
              {confirmingDelete ? t("profile.deleteConfirm") : t("common.delete")}
            </button>
          )}
          <span className="grow" />
          <button type="button" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="submit" className="primary" disabled={saving}>
            {t("common.save")}
          </button>
        </footer>
      </form>
    </div>
  );
}
