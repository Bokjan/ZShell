import { useId } from "react";
import { useTranslation } from "react-i18next";

import type { FoundShell, LocalShell, Profile, Protocol, SerialPortInfo } from "../../ipc/api";
import { foundName, sameShell } from "../../lib/localShells";
import { expandHome, startsWithHome } from "../../lib/paths";
import { isMac, isWindows } from "../../lib/platform";
import { shellOf, type ProfileForm } from "../../lib/profileForm";
import { AutoTextarea } from "../AutoTextarea";
import { ComboBox } from "../ComboBox";
import { HelpTip } from "../HelpTip";
import { IconButton } from "../IconButton";
import { RefreshIcon } from "../icons";
import { PasswordField } from "../PasswordField";
import { Select } from "../Select";
import { Field } from "../Field";

const PROTOCOLS: Protocol[] = ["ssh", "telnet", "serial", "local"];
/** Suggestions for the baud rate; any value can be typed. */
const BAUD_RATES = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600].map((rate) => ({ value: String(rate) }));
const PARITIES = ["none", "odd", "even"] as const;
const FLOW_CONTROLS = ["none", "software", "hardware"] as const;

const NAME_PLACEHOLDERS = {
  ssh: "profile.namePlaceholder",
  telnet: "profile.namePlaceholderTelnet",
  serial: "profile.namePlaceholderSerial",
  local: "profile.namePlaceholderLocal",
} as const;

/** The choice of a local terminal's program that is none of those found: another program. */
const OTHER_PROGRAM = "program";

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
  /** Whether the key file named isn't on this computer (saving it is still allowed). */
  keyMissing: boolean;
  /** The serial ports found here; null until listed. */
  ports: SerialPortInfo[] | null;
  refreshPorts(): void;
  /** Whether "local" can be chosen: not where the system blocks local terminals, unless it is already. */
  localAllowed: boolean;
  /** The shells found here, to run in a local terminal; null until known. */
  shells: FoundShell[] | null;
  chooseShell(shell: LocalShell): void;
  chooseProgram(): void;
  chooseStartDir(): void;
}

/** The session dialog's first page: the name, the protocol, and where and how it connects. */
export function GeneralPage({
  form,
  set,
  changeProtocol,
  profile,
  password,
  usesPassword,
  home,
  chooseKey,
  keyMissing,
  ports,
  refreshPorts,
  localAllowed,
  shells,
  chooseShell,
  chooseProgram,
  chooseStartDir,
}: Props) {
  const { t } = useTranslation();
  const keyMissingId = useId();
  const { protocol, authType } = form;
  const ssh = protocol === "ssh";

  const address = (
    <div className="row">
      <Field className="grow" title={t("profile.host")}>
        <input
          name="host"
          value={form.host}
          onChange={(e) => set({ host: e.target.value })}
          placeholder="example.com"
          autoFocus
          autoCapitalize="off"
          autoCorrect="off"
        />
      </Field>
      <Field className="port" title={t("profile.port")}>
        <input name="port" value={form.port} onChange={(e) => set({ port: e.target.value })} inputMode="numeric" />
      </Field>
    </div>
  );

  const usernameField = (
    <Field title={t("profile.username")}>
      <input
        name="username"
        value={form.username}
        onChange={(e) => set({ username: e.target.value })}
        placeholder={ssh ? undefined : t("profile.optional")}
        autoCapitalize="off"
        autoCorrect="off"
      />
    </Field>
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
      <Field title={t("profile.auth")}>
        <Select
          value={authType}
          onChange={(authType) => set({ authType })}
          options={[
            { value: "auto", label: t("profile.authAuto") },
            { value: "password", label: t("profile.authPassword") },
            { value: "publicKey", label: t("profile.authPublicKey") },
            { value: "agent", label: t("profile.authAgent") },
          ]}
        />
      </Field>
      {authType === "auto" && (
        <p className="hint">
          {t("profile.autoHint")}
          {home && <HelpTip text={t("profile.homeTip", { home })} />}
        </p>
      )}
      {usesPassword && passwordField}
      {authType === "publicKey" && (
        <>
          <Field
            title={
              <>
                {t("profile.keyPath")}
                {home && startsWithHome(form.keyPath.trim()) && (
                  <HelpTip text={t("profile.keyPathTip", { path: expandHome(form.keyPath.trim(), home) })} />
                )}
              </>
            }
          >
            <div className="input-with-button">
              <input
                name="keyPath"
                value={form.keyPath}
                onChange={(e) => set({ keyPath: e.target.value })}
                aria-describedby={keyMissing ? keyMissingId : undefined}
              />
              <button type="button" className="secondary" onClick={chooseKey}>
                {t("profile.chooseKeyButton")}
              </button>
            </div>
          </Field>
          {keyMissing && (
            <p className="hint warning" id={keyMissingId}>
              {t("profile.keyMissing")}
            </p>
          )}
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
      <Field title={t("profile.device")}>
        <div className="input-with-button">
          <ComboBox
            name="device"
            value={form.device}
            onChange={(device) => set({ device })}
            suggestions={(ports ?? []).map((port) => ({ value: port.name, description: port.description ?? undefined }))}
            placeholder={isWindows ? "COM3" : isMac ? "/dev/cu.usbserial-1410" : "/dev/ttyUSB0"}
            autoFocus
            autoCapitalize="off"
            autoCorrect="off"
          />
          <IconButton type="button" className="icon-button" label={t("profile.refreshDevices")} onClick={refreshPorts}>
            <RefreshIcon />
          </IconButton>
        </div>
      </Field>
      {ports?.length === 0 && <p className="hint">{t("profile.noDevices")}</p>}
      <div className="row">
        <Field className="grow" title={t("profile.baudRate")}>
          <ComboBox
            name="baudRate"
            value={form.baudRate}
            onChange={(baudRate) => set({ baudRate })}
            suggestions={BAUD_RATES}
            inputMode="numeric"
          />
        </Field>
        <Field className="grow" title={t("profile.dataBits")}>
          <Select
            value={form.dataBits}
            onChange={(dataBits) => set({ dataBits })}
            options={[8, 7, 6, 5].map((bits) => ({ value: bits, label: String(bits) }))}
          />
        </Field>
      </div>
      <div className="row">
        <Field className="grow" title={t("profile.parity")}>
          <Select
            value={form.parity}
            onChange={(parity) => set({ parity })}
            options={PARITIES.map((value) => ({ value, label: t(`profile.parities.${value}`) }))}
          />
        </Field>
        <Field className="grow" title={t("profile.stopBits")}>
          <Select
            value={form.stopBits}
            onChange={(stopBits) => set({ stopBits })}
            options={[1, 2].map((bits) => ({ value: bits, label: String(bits) }))}
          />
        </Field>
      </div>
      <Field title={t("profile.flowControl")} hint={t("profile.serialHint")}>
        <Select
          value={form.flowControl}
          onChange={(flowControl) => set({ flowControl })}
          options={FLOW_CONTROLS.map((value) => ({ value, label: t(`profile.flowControls.${value}`) }))}
        />
      </Field>
    </>
  );

  // The program chosen: the default shell, one of those found, or another one (a WSL
  // distribution not found here, from another computer, stays one).
  const shell = shellOf(form);
  const foundIndex = (shells ?? []).findIndex((found) => sameShell(found.shell, shell));
  const choice = form.localShell === "default" ? "default" : foundIndex >= 0 ? `found:${foundIndex}` : form.localShell;
  const distros = (shells ?? []).flatMap((found) => (found.shell.kind === "wsl" ? [{ value: found.shell.distro }] : []));

  const localFields = (
    <>
      <Field
        title={t("profile.shell")}
        hint={choice === "default" ? t(isWindows ? "profile.defaultShellHintWindows" : "profile.defaultShellHint") : undefined}
      >
        <Select
          value={choice}
          onChange={(value) => {
            if (value === "default") chooseShell({ kind: "default" });
            else if (value === OTHER_PROGRAM) set({ localShell: "program", program: "", args: "" });
            else if (value.startsWith("found:") && shells) chooseShell(shells[Number(value.slice(6))].shell);
          }}
          options={[
            { value: "default", label: t("profile.defaultShell") },
            ...(shells ?? []).map((found, i) => ({ value: `found:${i}`, label: foundName(found, t) })),
            ...(choice === "wsl" ? [{ value: "wsl", label: t("profile.distro") }] : []),
            { value: OTHER_PROGRAM, label: t("profile.otherProgram") },
          ]}
        />
      </Field>
      {form.localShell === "program" && (
        <>
          <Field title={t("profile.program")}>
            <div className="input-with-button">
              <input
                name="program"
                value={form.program}
                onChange={(e) => set({ program: e.target.value })}
                placeholder={isWindows ? "C:\\Program Files\\Git\\bin\\bash.exe" : "/bin/zsh"}
                autoFocus={choice === OTHER_PROGRAM && !form.program}
                autoCapitalize="off"
                autoCorrect="off"
              />
              <button type="button" className="secondary" onClick={chooseProgram}>
                {t("profile.chooseProgramButton")}
              </button>
            </div>
          </Field>
          <Field title={t("profile.args")} hint={t("profile.argsHint")}>
            <input name="args" value={form.args} onChange={(e) => set({ args: e.target.value })} autoCapitalize="off" autoCorrect="off" />
          </Field>
        </>
      )}
      {form.localShell === "wsl" && (
        <Field title={t("profile.distro")}>
          <ComboBox
            name="distro"
            value={form.distro}
            onChange={(distro) => set({ distro })}
            suggestions={distros}
            autoCapitalize="off"
            autoCorrect="off"
          />
        </Field>
      )}
      <Field title={t("profile.startDir")} hint={t(form.localShell === "wsl" ? "profile.startDirHintWsl" : "profile.startDirHint")}>
        <div className="input-with-button">
          <input
            name="startDir"
            value={form.startDir}
            onChange={(e) => set({ startDir: e.target.value })}
            placeholder="~"
            autoCapitalize="off"
            autoCorrect="off"
          />
          <button type="button" className="secondary" onClick={chooseStartDir}>
            {t("profile.chooseStartDirButton")}
          </button>
        </div>
      </Field>
    </>
  );

  const protocols = PROTOCOLS.filter((value) => value !== "local" || localAllowed || protocol === "local");
  const fields = { ssh: sshFields, telnet: telnetFields, serial: serialFields, local: localFields };

  return (
    <>
      <div className="row">
        <Field className="grow" title={t("profile.name")}>
          <input value={form.name} onChange={(e) => set({ name: e.target.value })} placeholder={t(NAME_PLACEHOLDERS[protocol])} />
        </Field>
        <Field title={t("profile.protocol")}>
          <Select
            value={protocol}
            onChange={changeProtocol}
            options={protocols.map((value) => ({ value, label: t(`profile.protocols.${value}`) }))}
          />
        </Field>
      </div>
      {fields[protocol]}
      <Field title={t("profile.notes")} hint={t("profile.notesHint")}>
        <AutoTextarea
          rows={3}
          value={form.notes}
          onChange={(e) => set({ notes: e.target.value })}
          placeholder={t("profile.notesPlaceholder")}
        />
      </Field>
    </>
  );
}
