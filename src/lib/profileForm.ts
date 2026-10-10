import type { TFunction } from "i18next";

import {
  DEFAULT_GROUP,
  DEFAULT_SERIAL,
  DEFAULT_TERM_TYPE,
  profileForwards,
  type AuthMethod,
  type Connection,
  type EnvVar,
  type FlowControl,
  type Parity,
  type Profile,
  type ProfileAppearance,
  type Protocol,
  type Remote,
} from "./api";
import { parsePort, wholeNumber } from "./format";
import { FONT_SIZE_MAX, FONT_SIZE_MIN } from "./settings";

/** Values to start a new profile with: from a quick connection, or the folder it goes in. */
export interface ProfileDefaults {
  protocol?: Protocol;
  host?: string;
  port?: number;
  username?: string;
  folder?: string;
}

export const PAGES = ["general", "connection", "terminal", "appearance"] as const;
export type Page = (typeof PAGES)[number];

/**
 * The session dialog's fields, as typed. Those of every protocol are kept while another is
 * chosen; only the chosen one's are checked and saved.
 */
export interface ProfileForm {
  name: string;
  protocol: Protocol;
  host: string;
  port: string;
  username: string;
  authType: AuthMethod["type"];
  keyPath: string;
  jumpHosts: string[];
  proxy: string;
  keepalive: string;
  autoReconnect: boolean;
  forwardAgent: boolean;
  encoding: string;
  termType: string;
  /** `NAME=value` lines. */
  env: string;
  /** One command per line. */
  loginCommands: string;
  autoLog: boolean;
  commandGroup: string;
  colorScheme: string;
  customBackground: boolean;
  background: string;
  fontFamily: string;
  fontSize: string;
  device: string;
  baudRate: string;
  dataBits: number;
  parity: Parity;
  stopBits: number;
  flowControl: FlowControl;
}

export type Field = keyof ProfileForm;

/** The color picker's starting value before a background is chosen. */
const DEFAULT_BACKGROUND = "#5a1414";

const envText = (env: EnvVar[]) => env.map((v) => `${v.name}=${v.value}`).join("\n");

/** `NAME=value` lines (blank ones skipped); the first line without `=` instead. */
export function parseEnv(text: string): EnvVar[] | { invalid: string } {
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

/** The form for `profile`, or for a new one (null) with `defaults`. */
export function fromProfile(profile: Profile | null, defaults: ProfileDefaults = {}): ProfileForm {
  const current = profile?.connection;
  const remote = current && current.protocol !== "serial" ? current : undefined;
  const ssh = current?.protocol === "ssh" ? current : undefined;
  const serial = current?.protocol === "serial" ? current : DEFAULT_SERIAL;
  const own = profile?.appearance;
  return {
    name: profile?.name ?? "",
    protocol: current?.protocol ?? defaults.protocol ?? "ssh",
    host: remote?.host ?? defaults.host ?? "",
    port: String(remote?.port ?? defaults.port ?? 22),
    username: remote?.username ?? defaults.username ?? "",
    authType: ssh?.auth.type ?? "auto",
    keyPath: ssh?.auth.type === "publicKey" ? ssh.auth.keyPath : "~/.ssh/id_ed25519",
    jumpHosts: remote?.jumpHosts ?? [],
    proxy: remote?.proxy ?? "",
    keepalive: String(remote?.keepaliveInterval ?? 30),
    autoReconnect: profile?.autoReconnect ?? true,
    forwardAgent: ssh?.forwardAgent ?? false,
    encoding: profile?.encoding ?? "utf-8",
    termType: remote?.termType ?? DEFAULT_TERM_TYPE,
    env: envText(ssh?.env ?? []),
    loginCommands: (profile?.loginCommands ?? []).join("\n"),
    autoLog: profile?.autoLog ?? false,
    commandGroup: profile?.commandGroup ?? DEFAULT_GROUP,
    colorScheme: own?.colorScheme ?? "",
    customBackground: !!own?.background,
    background: own?.background ?? DEFAULT_BACKGROUND,
    fontFamily: own?.fontFamily ?? "",
    fontSize: own?.fontSize ? String(own.fontSize) : "",
    device: serial.device,
    baudRate: String(serial.baudRate),
    dataBits: serial.dataBits,
    parity: serial.parity,
    stopBits: serial.stopBits,
    flowControl: serial.flowControl,
  };
}

/** Whether the form differs from what it started as (a question before throwing it away). */
export const isChanged = (form: ProfileForm, initial: ProfileForm) => JSON.stringify(form) !== JSON.stringify(initial);

/** What the form makes of the session's appearance. */
export const appearanceOf = (form: ProfileForm): ProfileAppearance => ({
  colorScheme: form.colorScheme || undefined,
  background: form.customBackground ? form.background : undefined,
  fontFamily: form.fontFamily.trim() || undefined,
  fontSize: form.fontSize.trim() ? wholeNumber(form.fontSize) : undefined,
});

/** A field that can't be saved as it is: where it is, and why. */
export interface Problem {
  page: Page;
  field: Field;
  message: string;
}

/**
 * The first field that can't be saved as it is, in the order of the pages; null if none.
 * Fields of other protocols aren't checked.
 */
export function validate(form: ProfileForm, t: TFunction): Problem | null {
  const { protocol } = form;
  const ssh = protocol === "ssh";
  const problem = (page: Page, field: Field, message: string): Problem => ({ page, field, message });
  if (protocol === "serial") {
    if (!form.device.trim()) return problem("general", "device", t("profile.missingDevice"));
    const baud = wholeNumber(form.baudRate);
    // The backend takes a 32-bit number.
    if (!(baud >= 1 && baud <= 0xffffffff)) return problem("general", "baudRate", t("profile.invalidBaudRate"));
  } else {
    if (!form.host.trim()) return problem("general", "host", t(ssh ? "profile.missingFields" : "profile.missingHost"));
    if (parsePort(form.port) === null) return problem("general", "port", t("profile.invalidPort"));
    if (ssh && !form.username.trim()) return problem("general", "username", t("profile.missingFields"));
    if (ssh && form.authType === "publicKey" && !form.keyPath.trim()) return problem("general", "keyPath", t("profile.missingFields"));
    const keepalive = wholeNumber(form.keepalive);
    if (!(keepalive >= 0 && keepalive <= 3600)) return problem("connection", "keepalive", t("profile.invalidKeepalive"));
    const env = parseEnv(form.env);
    if (ssh && !Array.isArray(env)) return problem("terminal", "env", t("profile.invalidEnvLine", { line: env.invalid }));
  }
  const size = appearanceOf(form).fontSize;
  if (size !== undefined && (!Number.isInteger(size) || size < FONT_SIZE_MIN || size > FONT_SIZE_MAX)) {
    return problem("appearance", "fontSize", t("profile.invalidFontSize", { min: FONT_SIZE_MIN, max: FONT_SIZE_MAX }));
  }
  return null;
}

/**
 * The profile to save from a valid form (see `validate`): `base` is the profile edited (its
 * id, folder and forwarding rules are kept), null for a new one going into `folder`.
 * `commandGroup` is the group shown, which may be the default for one deleted since.
 */
export function toProfile(form: ProfileForm, base: Profile | null, folder: string | undefined, commandGroup: string): Profile {
  const { protocol } = form;
  const remote: Remote = {
    host: form.host,
    port: parsePort(form.port) ?? 0,
    username: form.username,
    jumpHosts: form.jumpHosts,
    // The backend drops it with jump hosts.
    proxy: form.proxy && form.jumpHosts.length === 0 ? form.proxy : undefined,
    keepaliveInterval: wholeNumber(form.keepalive),
    termType: form.termType,
  };
  let connection: Connection;
  if (protocol === "serial") {
    const { device, dataBits, parity, stopBits, flowControl } = form;
    connection = { protocol, device, baudRate: wholeNumber(form.baudRate), dataBits, parity, stopBits, flowControl };
  } else if (protocol === "telnet") {
    connection = { protocol, ...remote };
  } else {
    const env = parseEnv(form.env);
    const auth: AuthMethod = form.authType === "publicKey" ? { type: "publicKey", keyPath: form.keyPath.trim() } : { type: form.authType };
    // Forwarding rules are edited in the forwards panel; the backend keeps the saved ones.
    const forwards = base ? profileForwards(base) : [];
    connection = { protocol, ...remote, auth, forwardAgent: form.forwardAgent, env: Array.isArray(env) ? env : [], forwards };
  }
  return {
    id: base?.id ?? "",
    name: form.name,
    connection,
    autoReconnect: form.autoReconnect,
    encoding: form.encoding,
    loginCommands: form.loginCommands.split("\n"),
    appearance: appearanceOf(form),
    autoLog: form.autoLog,
    commandGroup: commandGroup === DEFAULT_GROUP ? undefined : commandGroup,
    // Where a new profile goes; the backend keeps an existing one's folder.
    folder: base?.folder ?? folder,
  };
}

/** The new protocol, with the port following it unless it was changed from a usual one. */
export function withProtocol(form: ProfileForm, protocol: Protocol, defaultPorts: Record<"ssh" | "telnet", number>): ProfileForm {
  const usual = Object.values(defaultPorts).map(String);
  const port = protocol !== "serial" && usual.includes(form.port) ? String(defaultPorts[protocol]) : form.port;
  return { ...form, protocol, port };
}
