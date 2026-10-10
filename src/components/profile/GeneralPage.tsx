import { useTranslation } from "react-i18next";

import type { Profile, Protocol, SerialPortInfo } from "../../lib/api";
import { expandHome, startsWithHome } from "../../lib/paths";
import { isWindows } from "../../lib/platform";
import type { ProfileForm } from "../../lib/profileForm";
import { HelpTip } from "../HelpTip";
import { IconButton } from "../IconButton";
import { RefreshIcon } from "../icons";
import { PasswordField } from "../PasswordField";

const PROTOCOLS: Protocol[] = ["ssh", "telnet", "serial"];
/** Suggestions for the baud rate; any value can be typed. */
const BAUD_RATES = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const PARITIES = ["none", "odd", "even"] as const;
const FLOW_CONTROLS = ["none", "software", "hardware"] as const;

const NAME_PLACEHOLDERS = { ssh: "profile.namePlaceholder", telnet: "profile.namePlaceholderTelnet", serial: "profile.namePlaceholderSerial" } as const;

interface Props {
  form: ProfileForm;
  set(patch: Partial<ProfileForm>): void;
  changeProtocol(protocol: Protocol): void;
  /** The profile edited; null for a new one. */
  profile: Profile | null;
  password: { value: string; set(value: string): void; clear: boolean; setClear(clear: boolean): void };
  /** Whether the session keeps a password (see `ProfileDialog`). */
  usesPassword: boolean;
  /** The user's home folder, which `~` stands for in key paths. */
  home: string | null;
  chooseKey(): void;
  /** The serial ports found here; null until listed. */
  ports: SerialPortInfo[] | null;
  refreshPorts(): void;
}

/** The session dialog's first page: the name, the protocol, and where and how it connects. */
export function GeneralPage({ form, set, changeProtocol, profile, password, usesPassword, home, chooseKey, ports, refreshPorts }: Props) {
  const { t } = useTranslation();
  const { protocol, authType } = form;
  const ssh = protocol === "ssh";

  const address = (
    <div className="row">
      <label className="grow">
        {t("profile.host")}
        <input
          name="host"
          value={form.host}
          onChange={(e) => set({ host: e.target.value })}
          placeholder="example.com"
          autoFocus
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
      </label>
      <label className="port">
        {t("profile.port")}
        <input name="port" value={form.port} onChange={(e) => set({ port: e.target.value })} inputMode="numeric" />
      </label>
    </div>
  );

  const usernameField = (
    <label>
      {t("profile.username")}
      <input
        name="username"
        value={form.username}
        onChange={(e) => set({ username: e.target.value })}
        placeholder={ssh ? undefined : t("profile.optional")}
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />
    </label>
  );

  const passwordField = (
    <PasswordField
      value={password.value}
      onChange={password.set}
      clear={password.clear}
      onClearChange={password.setClear}
      canClear={!!profile}
      placeholder={profile ? t("profile.passwordKeepPlaceholder") : ssh ? t("profile.passwordAskPlaceholder") : t("profile.optional")}
      hint={!ssh ? t("profile.telnetLoginHint") : authType === "auto" ? t("profile.passwordAutoHint") : t("profile.passwordHint")}
    />
  );

  const sshFields = (
    <>
      {address}
      {usernameField}
      <label>
        {t("profile.auth")}
        <select value={authType} onChange={(e) => set({ authType: e.target.value as ProfileForm["authType"] })}>
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
              {home && startsWithHome(form.keyPath.trim()) && (
                <HelpTip text={t("profile.keyPathTip", { path: expandHome(form.keyPath.trim(), home) })} />
              )}
            </span>
            <div className="input-with-button">
              <input name="keyPath" value={form.keyPath} onChange={(e) => set({ keyPath: e.target.value })} spellCheck={false} />
              <button type="button" className="secondary" onClick={chooseKey}>
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
            name="device"
            value={form.device}
            onChange={(e) => set({ device: e.target.value })}
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
          <input name="baudRate" value={form.baudRate} onChange={(e) => set({ baudRate: e.target.value })} list="profile-baud-rates" inputMode="numeric" />
          <datalist id="profile-baud-rates">
            {BAUD_RATES.map((rate) => (
              <option key={rate} value={rate} />
            ))}
          </datalist>
        </label>
        <label className="grow">
          {t("profile.dataBits")}
          <select value={form.dataBits} onChange={(e) => set({ dataBits: Number(e.target.value) })}>
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
          <select value={form.parity} onChange={(e) => set({ parity: e.target.value as ProfileForm["parity"] })}>
            {PARITIES.map((value) => (
              <option key={value} value={value}>
                {t(`profile.parities.${value}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="grow">
          {t("profile.stopBits")}
          <select value={form.stopBits} onChange={(e) => set({ stopBits: Number(e.target.value) })}>
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
        <select value={form.flowControl} onChange={(e) => set({ flowControl: e.target.value as ProfileForm["flowControl"] })}>
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

  return (
    <>
      <div className="row">
        <label className="grow">
          {t("profile.name")}
          <input value={form.name} onChange={(e) => set({ name: e.target.value })} placeholder={t(NAME_PLACEHOLDERS[protocol])} />
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
}
