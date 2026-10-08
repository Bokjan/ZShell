import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  DEFAULT_GROUP,
  DEFAULT_TERM_TYPE,
  ENCODINGS,
  deleteProfile,
  errorMessage,
  saveProfile,
  type AuthMethod,
  type CommandGroup,
  type EnvVar,
  type Profile,
  type ProfileAppearance,
} from "../lib/api";
import { groupName } from "../lib/quickCommands";
import { FONT_SIZE_MAX, FONT_SIZE_MIN, useSettings } from "../lib/settings";
import { TERMINAL_SCHEMES, sessionScheme } from "../lib/terminalSchemes";
import { SchemePreview, schemeLabel } from "./SchemePreview";

/** Values to start a new profile with: from a quick connection, or the folder it goes in. */
export type ProfileDefaults = Partial<Pick<Profile, "host" | "port" | "username" | "folder">>;

interface Props {
  /** null creates a new profile. */
  profile: Profile | null;
  defaults?: ProfileDefaults;
  /** All profiles, to pick jump hosts from. */
  profiles: Profile[];
  /** To pick the quick command group its tabs show first. */
  commandGroups: CommandGroup[];
  onClose(): void;
  onChanged(): void;
  /** Called with the saved profile (not on delete). */
  onSaved?(profile: Profile): void;
}

type AuthType = AuthMethod["type"];

const PAGES = ["general", "connection", "terminal", "appearance"] as const;
type Page = (typeof PAGES)[number];

/** Suggestions for the terminal type; any value can be typed. */
const TERM_TYPES = [DEFAULT_TERM_TYPE, "xterm", "vt100", "vt220", "linux"];

/** The color picker's starting value before a background is chosen. */
const DEFAULT_BACKGROUND = "#5a1414";

const envText = (env: EnvVar[]) => env.map((v) => `${v.name}=${v.value}`).join("\n");

/** `NAME=value` lines (blank ones skipped); the first line without `=` instead. */
function parseEnv(text: string): EnvVar[] | { invalid: string } {
  const vars: EnvVar[] = [];
  for (const raw of text.split("\n")) {
    const line = raw.replace(/\r$/, "").trimStart();
    if (!line.trim()) continue;
    const eq = line.indexOf("=");
    if (eq <= 0) return { invalid: line };
    vars.push({ name: line.slice(0, eq), value: line.slice(eq + 1) });
  }
  return vars;
}

export function ProfileDialog({ profile, defaults, profiles, commandGroups, onClose, onChanged, onSaved }: Props) {
  const { t } = useTranslation();
  const { settings, theme } = useSettings();
  const [page, setPage] = useState<Page>("general");
  const [name, setName] = useState(profile?.name ?? "");
  const [host, setHost] = useState(profile?.host ?? defaults?.host ?? "");
  const [port, setPort] = useState(String(profile?.port ?? defaults?.port ?? 22));
  const [username, setUsername] = useState(profile?.username ?? defaults?.username ?? "");
  const [authType, setAuthType] = useState<AuthType>(profile?.auth.type ?? "auto");
  const [keyPath, setKeyPath] = useState(
    profile?.auth.type === "publicKey" ? profile.auth.keyPath : "~/.ssh/id_ed25519",
  );
  const [jumpHosts, setJumpHosts] = useState<string[]>(profile?.jumpHosts ?? []);
  const [keepalive, setKeepalive] = useState(String(profile?.keepaliveInterval ?? 30));
  const [autoReconnect, setAutoReconnect] = useState(profile?.autoReconnect ?? true);
  const [forwardAgent, setForwardAgent] = useState(profile?.forwardAgent ?? false);
  const [encoding, setEncoding] = useState(profile?.encoding ?? "utf-8");
  const [termType, setTermType] = useState(profile?.termType ?? DEFAULT_TERM_TYPE);
  const [env, setEnv] = useState(envText(profile?.env ?? []));
  const [loginCommands, setLoginCommands] = useState((profile?.loginCommands ?? []).join("\n"));
  const [autoLog, setAutoLog] = useState(profile?.autoLog ?? false);
  // A group deleted since falls back to the default group, as its tabs do.
  const [commandGroup, setCommandGroup] = useState(() =>
    commandGroups.some((group) => group.id === profile?.commandGroup) ? profile!.commandGroup! : DEFAULT_GROUP,
  );
  const own = profile?.appearance;
  const [colorScheme, setColorScheme] = useState(own?.colorScheme ?? "");
  const [customBackground, setCustomBackground] = useState(!!own?.background);
  const [background, setBackground] = useState(own?.background ?? DEFAULT_BACKGROUND);
  const [fontFamily, setFontFamily] = useState(own?.fontFamily ?? "");
  const [fontSize, setFontSize] = useState(own?.fontSize ? String(own.fontSize) : "");
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Automatic authentication falls back to a password, so it can keep a stored one too.
  const usesPassword = authType === "password" || authType === "auto";
  const profileName = (id: string) => profiles.find((p) => p.id === id)?.name ?? id;
  const jumpCandidates = profiles.filter((p) => p.id !== profile?.id && !jumpHosts.includes(p.id));

  const appearance: ProfileAppearance = {
    colorScheme: colorScheme || undefined,
    background: customBackground ? background : undefined,
    fontFamily: fontFamily.trim() || undefined,
    fontSize: fontSize.trim() ? Number(fontSize) : undefined,
  };
  const previewScheme = sessionScheme(settings.terminal.colorScheme, theme, appearance);

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

  /** Shows a problem with a field, on the page the field is on. */
  const invalid = (on: Page, message: string) => {
    setPage(on);
    setError(message);
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    // Checked here rather than with `required`: the field may be on another page.
    if (!host.trim() || !username.trim() || (authType === "publicKey" && !keyPath.trim())) {
      invalid("general", t("profile.missingFields"));
      return;
    }
    const portNumber = Number(port);
    if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
      invalid("general", t("profile.invalidPort"));
      return;
    }
    const keepaliveInterval = Number(keepalive);
    if (keepalive.trim() === "" || !Number.isInteger(keepaliveInterval) || keepaliveInterval < 0 || keepaliveInterval > 3600) {
      invalid("connection", t("profile.invalidKeepalive"));
      return;
    }
    const envVars = parseEnv(env);
    if (!Array.isArray(envVars)) {
      invalid("terminal", t("profile.invalidEnvLine", { line: envVars.invalid }));
      return;
    }
    const size = appearance.fontSize;
    if (size !== undefined && (!Number.isInteger(size) || size < FONT_SIZE_MIN || size > FONT_SIZE_MAX)) {
      invalid("appearance", t("profile.invalidFontSize", { min: FONT_SIZE_MIN, max: FONT_SIZE_MAX }));
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
      const saved = await saveProfile(
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
          forwardAgent,
          encoding,
          termType,
          env: envVars,
          loginCommands: loginCommands.split("\n"),
          appearance,
          autoLog,
          commandGroup: commandGroup === DEFAULT_GROUP ? undefined : commandGroup,
          forwards: profile?.forwards ?? [],
          // Where a new profile goes; the backend keeps an existing one's folder.
          folder: profile?.folder ?? defaults?.folder,
        },
        passwordUpdate,
      );
      onChanged();
      onSaved?.(saved);
      onClose();
    } catch (err) {
      // The profile itself may have been saved with only the password change failing (the
      // keychain), so the list is reloaded either way.
      onChanged();
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

  const general = (
    <>
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
              placeholder={profile ? t("profile.passwordKeepPlaceholder") : t("profile.passwordAskPlaceholder")}
            />
          </label>
          <p className="hint">{authType === "auto" ? t("profile.passwordAutoHint") : t("profile.passwordHint")}</p>
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
            <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} spellCheck={false} />
          </label>
          <p className="hint">{t("profile.keyHint")}</p>
        </>
      )}
      {authType === "agent" && <p className="hint">{t("profile.agentHint")}</p>}
    </>
  );

  const connection = (
    <>
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
      <label className="checkbox">
        <input type="checkbox" checked={forwardAgent} onChange={(e) => setForwardAgent(e.target.checked)} />
        {t("profile.forwardAgent")}
      </label>
      <p className="hint">{t("profile.forwardAgentHint")}</p>
    </>
  );

  const terminal = (
    <>
      <div className="row">
        <label className="grow">
          {t("profile.encoding")}
          <select value={encoding} onChange={(e) => setEncoding(e.target.value)}>
            {ENCODINGS.map((label) => (
              <option key={label} value={label}>
                {t(`profile.encodings.${label}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="grow">
          {t("profile.termType")}
          <input
            value={termType}
            onChange={(e) => setTermType(e.target.value)}
            list="profile-term-types"
            placeholder={DEFAULT_TERM_TYPE}
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
          <datalist id="profile-term-types">
            {TERM_TYPES.map((type) => (
              <option key={type} value={type} />
            ))}
          </datalist>
        </label>
      </div>
      <p className="hint">{t("profile.encodingHint")}</p>
      <label>
        {t("profile.loginCommands")}
        <textarea
          className="command-text"
          rows={3}
          value={loginCommands}
          onChange={(e) => setLoginCommands(e.target.value)}
          placeholder={t("profile.loginCommandsPlaceholder")}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </label>
      <p className="hint">{t("profile.loginCommandsHint")}</p>
      <label>
        {t("profile.env")}
        <textarea
          className="command-text"
          rows={2}
          value={env}
          onChange={(e) => setEnv(e.target.value)}
          placeholder={t("profile.envPlaceholder")}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
        />
      </label>
      <p className="hint">{t("profile.envHint")}</p>
      <label className="checkbox">
        <input type="checkbox" checked={autoLog} onChange={(e) => setAutoLog(e.target.checked)} />
        {t("profile.autoLog")}
      </label>
      <p className="hint">{t("profile.autoLogHint")}</p>
      <label>
        {t("profile.commandGroup")}
        <select value={commandGroup} onChange={(e) => setCommandGroup(e.target.value)}>
          {commandGroups.map((group) => (
            <option key={group.id} value={group.id}>
              {groupName(group, t)}
            </option>
          ))}
        </select>
      </label>
      <p className="hint">{t("profile.commandGroupHint")}</p>
    </>
  );

  const appearancePage = (
    <>
      <p className="hint profile-page-hint">{t("profile.appearanceHint")}</p>
      <div className="row profile-scheme">
        <label className="grow">
          {t("profile.colorScheme")}
          <select value={colorScheme} onChange={(e) => setColorScheme(e.target.value)}>
            <option value="">{t("profile.sameAsSettings")}</option>
            <option value="auto">{t("settings.schemeAuto")}</option>
            {TERMINAL_SCHEMES.map((scheme) => (
              <option key={scheme.id} value={scheme.id}>
                {schemeLabel(scheme, t)}
              </option>
            ))}
          </select>
        </label>
        <SchemePreview scheme={previewScheme} />
      </div>
      <div className="row profile-background">
        <label className="checkbox grow">
          <input type="checkbox" checked={customBackground} onChange={(e) => setCustomBackground(e.target.checked)} />
          {t("profile.customBackground")}
        </label>
        <input
          type="color"
          value={background}
          disabled={!customBackground}
          onChange={(e) => setBackground(e.target.value)}
          aria-label={t("profile.customBackground")}
        />
      </div>
      <p className="hint">{t("profile.customBackgroundHint")}</p>
      <div className="row">
        <label className="grow">
          {t("settings.fontFamily")}
          <input
            value={fontFamily}
            onChange={(e) => setFontFamily(e.target.value)}
            placeholder={settings.terminal.fontFamily || t("profile.sameAsSettings")}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </label>
        <label className="port">
          {t("settings.fontSize")}
          <input
            value={fontSize}
            onChange={(e) => setFontSize(e.target.value)}
            placeholder={String(settings.terminal.fontSize)}
            inputMode="numeric"
          />
        </label>
      </div>
    </>
  );

  const pages: Record<Page, ReactNode> = { general, connection, terminal, appearance: appearancePage };

  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="dialog profile-dialog" onSubmit={submit}>
        <h2>{profile ? t("profile.titleEdit") : t("profile.titleNew")}</h2>
        <div className="segmented profile-pages" role="tablist">
          {PAGES.map((p) => (
            <button
              key={p}
              type="button"
              role="tab"
              aria-selected={page === p}
              className={page === p ? "on" : undefined}
              onClick={() => setPage(p)}
            >
              {t(`profile.pages.${p}`)}
            </button>
          ))}
        </div>

        {/* All pages share one grid cell, so the dialog keeps the height of the tallest. */}
        <div className="profile-body">
          <div className="profile-stack">
            {PAGES.map((p) => (
              <div key={p} className={`profile-page${p === page ? "" : " hidden"}`} role="tabpanel" aria-hidden={p !== page}>
                {pages[p]}
              </div>
            ))}
          </div>
        </div>

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
