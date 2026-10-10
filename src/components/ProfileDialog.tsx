import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import {
  DEFAULT_GROUP,
  DEFAULT_PORTS,
  DEFAULT_SERIAL,
  DEFAULT_TERM_TYPE,
  ENCODINGS,
  deleteProfile,
  errorMessage,
  profileForwards,
  proxies as proxyApi,
  saveProfile,
  serialPorts,
  type AuthMethod,
  type CommandGroup,
  type Connection,
  type EnvVar,
  type FlowControl,
  type Parity,
  type Profile,
  type ProfileAppearance,
  type Protocol,
  type Proxy,
  type Remote,
  type SerialPortInfo,
} from "../lib/api";
import { useConfirmButton } from "../lib/confirm";
import { useDialog } from "../lib/dialogs";
import { wholeNumber } from "../lib/format";
import { contractHome, expandHome, startsWithHome, useHomeDirectory } from "../lib/paths";
import { isWindows } from "../lib/platform";
import { groupName } from "../lib/quickCommands";
import { FONT_SIZE_MAX, FONT_SIZE_MIN, useSettings } from "../lib/settings";
import { useSubmitting } from "../lib/submitting";
import { TERMINAL_SCHEMES, sessionScheme } from "../lib/terminalSchemes";
import { ErrorText } from "./ErrorMessage";
import { HelpTip } from "./HelpTip";
import { IconButton } from "./IconButton";
import { ArrowIcon, CloseIcon, RefreshIcon } from "./icons";
import { Modal } from "./Modal";
import { ProxyDialog } from "./ProxyDialog";
import { SchemePreview, schemeLabel } from "./SchemePreview";
import { SpinInput } from "./SpinInput";

/** Values to start a new profile with: from a quick connection, or the folder it goes in. */
export interface ProfileDefaults {
  protocol?: Protocol;
  host?: string;
  port?: number;
  username?: string;
  folder?: string;
}

interface Props {
  /** null creates a new profile. */
  profile: Profile | null;
  defaults?: ProfileDefaults;
  /** All profiles, to pick jump hosts from. */
  profiles: Profile[];
  /** To pick the quick command group its tabs show first; null until they have loaded. */
  commandGroups: CommandGroup[] | null;
  onClose(): void;
  onChanged(): void;
  /** Called with the saved profile (not on delete). */
  onSaved?(profile: Profile): void;
}

type AuthType = AuthMethod["type"];

const PAGES = ["general", "connection", "terminal", "appearance"] as const;
type Page = (typeof PAGES)[number];

const PROTOCOLS: Protocol[] = ["ssh", "telnet", "serial"];

/** Suggestions for the baud rate; any value can be typed. */
const BAUD_RATES = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const PARITIES: Parity[] = ["none", "odd", "even"];
const FLOW_CONTROLS: FlowControl[] = ["none", "software", "hardware"];

/** Suggestions for the terminal type; any value can be typed. */
const TERM_TYPES = [DEFAULT_TERM_TYPE, "xterm", "vt100", "vt220", "linux"];

/** The proxy choice that opens a dialog to create one. */
const NEW_PROXY = "\u0000new";

/** The color picker's starting value before a background is chosen. */
const DEFAULT_BACKGROUND = "#5a1414";

/** The proxy of an SSH or Telnet session. */
const proxyOf = (profile: Profile) => (profile.connection.protocol === "serial" ? undefined : profile.connection.proxy);

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

export function ProfileDialog({ profile: initial, defaults, profiles, commandGroups, onClose, onChanged, onSaved }: Props) {
  const { t } = useTranslation();
  // The profile being edited: a new one becomes the saved one when only its password failed,
  // so that saving again updates it instead of adding another.
  const [profile, setProfile] = useState(initial);
  // Saving and deleting, one at a time.
  const saving = useSubmitting();
  const { settings, theme } = useSettings();
  const [page, setPage] = useState<Page>("general");
  // The saved settings of the profile's protocol; those of the others start empty.
  const current = profile?.connection;
  const savedRemote = current && current.protocol !== "serial" ? current : undefined;
  const savedSsh = current?.protocol === "ssh" ? current : undefined;
  const [name, setName] = useState(profile?.name ?? "");
  const [protocol, setProtocol] = useState<Protocol>(current?.protocol ?? defaults?.protocol ?? "ssh");
  const [host, setHost] = useState(savedRemote?.host ?? defaults?.host ?? "");
  const [port, setPort] = useState(String(savedRemote?.port ?? defaults?.port ?? 22));
  const [username, setUsername] = useState(savedRemote?.username ?? defaults?.username ?? "");
  const [authType, setAuthType] = useState<AuthType>(savedSsh?.auth.type ?? "auto");
  const [keyPath, setKeyPath] = useState(savedSsh?.auth.type === "publicKey" ? savedSsh.auth.keyPath : "~/.ssh/id_ed25519");
  const [jumpHosts, setJumpHosts] = useState<string[]>(savedRemote?.jumpHosts ?? []);
  const [proxy, setProxy] = useState(savedRemote?.proxy ?? "");
  const [proxyList, setProxyList] = useState<Proxy[]>([]);
  const [creatingProxy, setCreatingProxy] = useState(false);
  const home = useHomeDirectory();
  const [keepalive, setKeepalive] = useState(String(savedRemote?.keepaliveInterval ?? 30));
  const [autoReconnect, setAutoReconnect] = useState(profile?.autoReconnect ?? true);
  const [forwardAgent, setForwardAgent] = useState(savedSsh?.forwardAgent ?? false);
  const [encoding, setEncoding] = useState(profile?.encoding ?? "utf-8");
  const [termType, setTermType] = useState(savedRemote?.termType ?? DEFAULT_TERM_TYPE);
  const [env, setEnv] = useState(envText(savedSsh?.env ?? []));
  const [loginCommands, setLoginCommands] = useState((profile?.loginCommands ?? []).join("\n"));
  const [autoLog, setAutoLog] = useState(profile?.autoLog ?? false);
  const [commandGroup, setCommandGroup] = useState(profile?.commandGroup ?? DEFAULT_GROUP);
  // A group deleted since counts as the default group, as its tabs do. Until the groups have
  // loaded (or if they couldn't), the profile's stays as it is.
  const shownGroup =
    !commandGroups || commandGroups.some((group) => group.id === commandGroup) ? commandGroup : DEFAULT_GROUP;
  const own = profile?.appearance;
  const [colorScheme, setColorScheme] = useState(own?.colorScheme ?? "");
  const [customBackground, setCustomBackground] = useState(!!own?.background);
  const [background, setBackground] = useState(own?.background ?? DEFAULT_BACKGROUND);
  const [fontFamily, setFontFamily] = useState(own?.fontFamily ?? "");
  const [fontSize, setFontSize] = useState(own?.fontSize ? String(own.fontSize) : "");
  const serial = current?.protocol === "serial" ? current : DEFAULT_SERIAL;
  const [device, setDevice] = useState(serial.device);
  const [baudRate, setBaudRate] = useState(String(serial.baudRate));
  const [dataBits, setDataBits] = useState(serial.dataBits);
  const [parity, setParity] = useState(serial.parity);
  const [stopBits, setStopBits] = useState(serial.stopBits);
  const [flowControl, setFlowControl] = useState(serial.flowControl);
  // The serial ports found here, listed once the serial protocol is chosen.
  const [ports, setPorts] = useState<SerialPortInfo[] | null>(null);
  const [password, setPassword] = useState("");
  const [clearPassword, setClearPassword] = useState(false);
  const deleteButton = useConfirmButton();
  const [error, setError] = useState<string | null>(null);
  const ssh = protocol === "ssh";
  // Automatic authentication falls back to a password, so it can keep a stored one too;
  // Telnet types it at the password prompt.
  const usesPassword = protocol === "telnet" || (ssh && (authType === "password" || authType === "auto"));
  const profileName = (id: string) => profiles.find((p) => p.id === id)?.name ?? id;
  // Only SSH sessions can be jump hosts.
  const jumpCandidates = profiles.filter(
    (p) => p.id !== profile?.id && p.connection.protocol === "ssh" && !jumpHosts.includes(p.id),
  );

  const refreshProxies = () => void proxyApi.list().then(setProxyList, console.error);
  useEffect(refreshProxies, []);
  const proxyName = (id: string | undefined) => proxyList.find((p) => p.id === id)?.name;
  // With jump hosts, the first one's own proxy is used.
  const firstJump = profiles.find((p) => p.id === jumpHosts[0]);

  // Starts in the folder of the current key, or in ~/.ssh; keys in the home folder are kept as
  // `~/…`, which works on other computers too (exported sessions).
  const chooseKey = async () => {
    const current = keyPath.trim();
    let start: string | undefined = current || "~/.ssh";
    if (startsWithHome(start)) start = home ? expandHome(start, home) : undefined;
    const picked = await openDialog({ title: t("profile.chooseKey"), defaultPath: start }).catch(() => null);
    if (typeof picked === "string") setKeyPath(home ? contractHome(picked, home) : picked);
  };

  const refreshPorts = () => void serialPorts().then(setPorts, () => setPorts([]));
  useEffect(() => {
    if (protocol === "serial" && ports === null) refreshPorts();
  }, [protocol, ports]);

  // The port follows the protocol unless it was changed from a usual one.
  const changeProtocol = (next: Protocol) => {
    const usual = Object.values(DEFAULT_PORTS).map(String);
    if (next !== "serial" && usual.includes(port)) setPort(String(DEFAULT_PORTS[next]));
    setProtocol(next);
  };

  const appearance: ProfileAppearance = {
    colorScheme: colorScheme || undefined,
    background: customBackground ? background : undefined,
    fontFamily: fontFamily.trim() || undefined,
    fontSize: fontSize.trim() ? wholeNumber(fontSize) : undefined,
  };
  const previewScheme = sessionScheme(settings.terminal.colorScheme, theme, appearance);

  const moveJumpHost = (index: number, offset: number) =>
    setJumpHosts((hosts) => {
      const next = [...hosts];
      [next[index], next[index + offset]] = [next[index + offset], next[index]];
      return next;
    });

  const dialog = useDialog(onClose);

  /** Shows a problem with a field, on the page the field is on. */
  const invalid = (on: Page, message: string) => {
    setPage(on);
    setError(message);
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    // Checked here rather than with `required`: the field may be on another page. Fields
    // of other protocols are neither checked nor saved.
    if (ssh && (!host.trim() || !username.trim() || (authType === "publicKey" && !keyPath.trim()))) {
      invalid("general", t("profile.missingFields"));
      return;
    }
    if (protocol === "telnet" && !host.trim()) {
      invalid("general", t("profile.missingHost"));
      return;
    }
    if (protocol === "serial" && !device.trim()) {
      invalid("general", t("profile.missingDevice"));
      return;
    }
    const portNumber = wholeNumber(port);
    if (protocol !== "serial" && (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535)) {
      invalid("general", t("profile.invalidPort"));
      return;
    }
    const baud = wholeNumber(baudRate);
    // The backend takes a 32-bit number.
    if (protocol === "serial" && !(baud >= 1 && baud <= 0xffffffff)) {
      invalid("general", t("profile.invalidBaudRate"));
      return;
    }
    const keepaliveInterval = wholeNumber(keepalive);
    if (protocol !== "serial" && !(keepaliveInterval >= 0 && keepaliveInterval <= 3600)) {
      invalid("connection", t("profile.invalidKeepalive"));
      return;
    }
    const envVars = parseEnv(env);
    if (ssh && !Array.isArray(envVars)) {
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
    const remote: Remote = {
      host,
      port: portNumber,
      username,
      jumpHosts,
      // The backend drops it with jump hosts.
      proxy: proxy && jumpHosts.length === 0 ? proxy : undefined,
      keepaliveInterval,
      termType,
    };
    let connection: Connection;
    if (protocol === "serial") connection = { protocol, device, baudRate: baud, dataBits, parity, stopBits, flowControl };
    else if (protocol === "telnet") connection = { protocol, ...remote };
    else {
      const sshEnv = Array.isArray(envVars) ? envVars : [];
      // Forwarding rules are edited in the forwards panel; the backend keeps the saved ones.
      const forwards = profile ? profileForwards(profile) : [];
      connection = { protocol, ...remote, auth, forwardAgent, env: sshEnv, forwards };
    }
    // Only password and automatic auth keep a stored password; switching away clears it.
    let passwordUpdate: string | undefined;
    if (!usesPassword) passwordUpdate = profile ? "" : undefined;
    else if (clearPassword) passwordUpdate = "";
    else if (password) passwordUpdate = password;

    await saving.submit(async () => {
      try {
        const { saved, passwordError } = await saveProfile(
          {
            id: profile?.id ?? "",
            name,
            connection,
            autoReconnect,
            encoding,
            loginCommands: loginCommands.split("\n"),
            appearance,
            autoLog,
            commandGroup: shownGroup === DEFAULT_GROUP ? undefined : shownGroup,
            // Where a new profile goes; the backend keeps an existing one's folder.
            folder: profile?.folder ?? defaults?.folder,
          },
          passwordUpdate,
        );
        onChanged();
        onSaved?.(saved);
        if (passwordError) {
          setProfile(saved);
          setError(t("profile.passwordNotSaved", { message: passwordError.message }));
          return;
        }
        onClose();
      } catch (err) {
        setError(errorMessage(err));
      }
    });
  };

  const remove = async () => {
    if (!profile) return;
    if (!deleteButton.armed) {
      deleteButton.setArmed(true);
      return;
    }
    await saving.submit(async () => {
      try {
        await deleteProfile(profile.id);
        onChanged();
        onClose();
      } catch (err) {
        setError(errorMessage(err));
      }
    });
  };

  const namePlaceholder = { ssh: "profile.namePlaceholder", telnet: "profile.namePlaceholderTelnet", serial: "profile.namePlaceholderSerial" } as const;

  const address = (
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
  );

  const usernameField = (
    <label>
      {t("profile.username")}
      <input
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        placeholder={ssh ? undefined : t("profile.optional")}
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />
    </label>
  );

  const passwordField = (
    <>
      <label>
        {t("profile.password")}
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          disabled={clearPassword}
          placeholder={
            profile ? t("profile.passwordKeepPlaceholder") : ssh ? t("profile.passwordAskPlaceholder") : t("profile.optional")
          }
        />
      </label>
      <p className="hint">
        {!ssh ? t("profile.telnetLoginHint") : authType === "auto" ? t("profile.passwordAutoHint") : t("profile.passwordHint")}
      </p>
      {profile && (
        <label className="checkbox">
          <input type="checkbox" checked={clearPassword} onChange={(e) => setClearPassword(e.target.checked)} />
          {t("profile.clearPassword")}
        </label>
      )}
    </>
  );

  const sshFields = (
    <>
      {address}
      {usernameField}
      <label>
        {t("profile.auth")}
        <select value={authType} onChange={(e) => setAuthType(e.target.value as AuthType)}>
          <option value="auto">{t("profile.authAuto")}</option>
          <option value="password">{t("profile.authPassword")}</option>
          <option value="publicKey">{t("profile.authPublicKey")}</option>
          <option value="agent">{t("profile.authAgent")}</option>
        </select>
      </label>

      {authType === "auto" && (
        <p className="hint">
          {t("profile.autoHint")}
          {home && <HelpTip text={t("profile.homeTip", { home })} />}
        </p>
      )}
      {usesPassword && passwordField}
      {authType === "publicKey" && (
        <>
          <label>
            <span>
              {t("profile.keyPath")}
              {home && startsWithHome(keyPath.trim()) && (
                <HelpTip text={t("profile.keyPathTip", { path: expandHome(keyPath.trim(), home) })} />
              )}
            </span>
            <div className="input-with-button">
              <input value={keyPath} onChange={(e) => setKeyPath(e.target.value)} spellCheck={false} />
              <button type="button" className="secondary" onClick={() => void chooseKey()}>
                {t("profile.chooseKeyButton")}
              </button>
            </div>
          </label>
          <p className="hint">{t("profile.keyHint")}</p>
        </>
      )}
      {authType === "agent" && <p className="hint">{t("profile.agentHint")}</p>}
    </>
  );

  const telnetFields = (
    <>
      {address}
      {usernameField}
      {passwordField}
    </>
  );

  const serialFields = (
    <>
      <label>
        {t("profile.device")}
        <div className="input-with-button">
          <input
            value={device}
            onChange={(e) => setDevice(e.target.value)}
            list="profile-serial-ports"
            placeholder={isWindows ? "COM3" : "/dev/cu.usbserial-1410"}
            autoFocus
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
          <IconButton type="button" className="icon-button" label={t("profile.refreshDevices")} onClick={refreshPorts}>
            <RefreshIcon />
          </IconButton>
        </div>
        <datalist id="profile-serial-ports">
          {ports?.map((port) => (
            <option key={port.name} value={port.name}>
              {port.description ?? undefined}
            </option>
          ))}
        </datalist>
      </label>
      {ports?.length === 0 && <p className="hint">{t("profile.noDevices")}</p>}
      <div className="row">
        <label className="grow">
          {t("profile.baudRate")}
          <input value={baudRate} onChange={(e) => setBaudRate(e.target.value)} list="profile-baud-rates" inputMode="numeric" />
          <datalist id="profile-baud-rates">
            {BAUD_RATES.map((rate) => (
              <option key={rate} value={rate} />
            ))}
          </datalist>
        </label>
        <label className="grow">
          {t("profile.dataBits")}
          <select value={dataBits} onChange={(e) => setDataBits(Number(e.target.value))}>
            {[8, 7, 6, 5].map((bits) => (
              <option key={bits} value={bits}>
                {bits}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="row">
        <label className="grow">
          {t("profile.parity")}
          <select value={parity} onChange={(e) => setParity(e.target.value as Parity)}>
            {PARITIES.map((value) => (
              <option key={value} value={value}>
                {t(`profile.parities.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="grow">
          {t("profile.stopBits")}
          <select value={stopBits} onChange={(e) => setStopBits(Number(e.target.value))}>
            {[1, 2].map((bits) => (
              <option key={bits} value={bits}>
                {bits}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label>
        {t("profile.flowControl")}
        <select value={flowControl} onChange={(e) => setFlowControl(e.target.value as FlowControl)}>
          {FLOW_CONTROLS.map((value) => (
            <option key={value} value={value}>
              {t(`profile.flowControls.${value}`)}
            </option>
          ))}
        </select>
      </label>
      <p className="hint">{t("profile.serialHint")}</p>
    </>
  );

  const general = (
    <>
      <div className="row">
        <label className="grow">
          {t("profile.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t(namePlaceholder[protocol])} />
        </label>
        <label>
          {t("profile.protocol")}
          <select value={protocol} onChange={(e) => changeProtocol(e.target.value as Protocol)}>
            {PROTOCOLS.map((value) => (
              <option key={value} value={value}>
                {t(`profile.protocols.${value}`)}
              </option>
            ))}
          </select>
        </label>
      </div>
      {protocol === "ssh" ? sshFields : protocol === "telnet" ? telnetFields : serialFields}
    </>
  );

  const connection = (
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
                      onClick={() => setJumpHosts((hosts) => hosts.filter((h) => h !== id))}
                    >
                      <CloseIcon size={12} />
                    </IconButton>
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
          <p className="hint">{t(ssh ? "profile.jumpHostsHint" : "profile.jumpHostsHintTelnet")}</p>
          <label>
            {t("profile.proxy")}
            <select
              value={firstJump ? (proxyOf(firstJump) ?? "") : proxy}
              disabled={!!firstJump}
              onChange={(e) => (e.target.value === NEW_PROXY ? setCreatingProxy(true) : setProxy(e.target.value))}
            >
              <option value="">{t("profile.noProxy")}</option>
              {proxyList.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
              <option value={NEW_PROXY}>{t("profile.newProxy")}</option>
            </select>
          </label>
          <p className="hint">
            {firstJump
              ? t("profile.proxyViaJumpHost", { name: firstJump.name, proxy: proxyName(proxyOf(firstJump)) ?? t("profile.noProxy") })
              : t("profile.proxyHint")}
          </p>
          <label>
            {t("profile.keepalive")}
            <input value={keepalive} onChange={(e) => setKeepalive(e.target.value)} inputMode="numeric" />
          </label>
          <p className="hint">{t(ssh ? "profile.keepaliveHint" : "profile.keepaliveHintTelnet")}</p>
        </>
      )}
      <label className="checkbox">
        <input type="checkbox" checked={autoReconnect} onChange={(e) => setAutoReconnect(e.target.checked)} />
        {t(protocol === "serial" ? "profile.autoReconnectSerial" : "profile.autoReconnect")}
      </label>
      {ssh && (
        <>
          <label className="checkbox">
            <input type="checkbox" checked={forwardAgent} onChange={(e) => setForwardAgent(e.target.checked)} />
            {t("profile.forwardAgent")}
          </label>
          <p className="hint">{t("profile.forwardAgentHint")}</p>
        </>
      )}
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
        {protocol !== "serial" && (
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
        )}
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
      {ssh && (
        <>
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
        </>
      )}
      <label className="checkbox">
        <input type="checkbox" checked={autoLog} onChange={(e) => setAutoLog(e.target.checked)} />
        {t("profile.autoLog")}
      </label>
      <p className="hint">{t("profile.autoLogHint")}</p>
      <label>
        {t("profile.commandGroup")}
        <select value={shownGroup} disabled={!commandGroups} onChange={(e) => setCommandGroup(e.target.value)}>
          {(commandGroups ?? []).map((group) => (
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
          <SpinInput
            value={fontSize}
            onChange={setFontSize}
            min={FONT_SIZE_MIN}
            max={FONT_SIZE_MAX}
            start={settings.terminal.fontSize}
            placeholder={String(settings.terminal.fontSize)}
          />
        </label>
      </div>
    </>
  );

  const pages: Record<Page, ReactNode> = { general, connection, terminal, appearance: appearancePage };

  return (
    <>
      <Modal dialog={dialog}>
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

          {error && <ErrorText>{error}</ErrorText>}

          <footer>
            {profile && (
              <button
                type="button"
                className="danger"
                ref={deleteButton.ref}
                onClick={remove}
                onBlur={deleteButton.onBlur}
                disabled={saving.busy}
              >
                {deleteButton.armed ? t("profile.deleteConfirm") : t("common.delete")}
              </button>
            )}
            <span className="grow" />
            <button type="button" onClick={onClose}>
              {t("common.cancel")}
            </button>
            <button type="submit" className="primary" disabled={saving.busy}>
              {t("common.save")}
            </button>
          </footer>
        </form>
      </Modal>
      {creatingProxy && (
        <ProxyDialog
          proxy={null}
          onClose={() => setCreatingProxy(false)}
          onSaved={(saved) => setProxy(saved.id)}
          onChanged={refreshProxies}
        />
      )}
    </>
  );
}
