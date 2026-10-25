// Generated from the Rust types and constants by `cargo test` (src-tauri/src/bindings.rs); do not edit.
// After changing them, run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri.

import type { Channel } from "@tauri-apps/api/core";

/** The `code` of a `CommandError`. */
export type ErrorCode =
  | "appLock.busy"
  | "appLock.cancelled"
  | "appLock.failed"
  | "appLock.gone"
  | "appLock.helloDisabled"
  | "appLock.helloNotSetUp"
  | "appLock.locked"
  | "appLock.policyMissing"
  | "appLock.polkitMissing"
  | "appLock.unavailable"
  | "appLock.unsupported"
  | "auth.agentAllRejected"
  | "auth.agentConnectFailed"
  | "auth.agentEmpty"
  | "auth.agentListFailed"
  | "auth.cancelled"
  | "auth.disconnected"
  | "auth.failed"
  | "auth.keyDecryptFailed"
  | "auth.keyReadFailed"
  | "auth.keyRejected"
  | "auth.moreRequired"
  | "auth.notOffered"
  | "auth.passwordFailed"
  | "colorPick.busy"
  | "colorPick.failed"
  | "colorPick.unavailable"
  | "colorPick.unsupported"
  | "config.unreadable"
  | "config.writeFailed"
  | "credentialStore"
  | "edit.chooseEditor"
  | "edit.conflict"
  | "edit.notFound"
  | "edit.openFailed"
  | "export.writeFailed"
  | "folder.emptyName"
  | "folder.invalidMove"
  | "folder.notFound"
  | "forward.acceptFailed"
  | "forward.bindFailed"
  | "forward.channelFailed"
  | "forward.channelTimeout"
  | "forward.missingTarget"
  | "forward.remoteRefused"
  | "forward.socksAddressType"
  | "forward.socksAuth"
  | "forward.socksCommand"
  | "forward.socksProtocol"
  | "forward.socksVersion"
  | "import.fileChanged"
  | "import.includeTooDeep"
  | "import.invalidProxy"
  | "import.invalidSession"
  | "import.notSessionsFile"
  | "import.parseFailed"
  | "import.passphraseRequired"
  | "import.readFailed"
  | "import.tooLarge"
  | "import.unsupportedEncryption"
  | "import.unsupportedVersion"
  | "import.wrongPassphrase"
  | "knownHosts.changed"
  | "knownHosts.noHome"
  | "knownHosts.readFailed"
  | "knownHosts.writeFailed"
  | "log.createFailed"
  | "log.timeFormatControl"
  | "log.timeFormatTooLong"
  | "log.timeFormatUnknown"
  | "log.writeFailed"
  | "net.connectFailed"
  | "net.connectTimeout"
  | "net.connectionLost"
  | "profile.forwardsNeedSsh"
  | "profile.invalidEncoding"
  | "profile.invalidEnvName"
  | "profile.invalidHost"
  | "profile.invalidJumpHost"
  | "profile.invalidProxy"
  | "profile.invalidSerialSettings"
  | "profile.invalidTermType"
  | "profile.invalidUser"
  | "profile.jumpHostNotSsh"
  | "profile.missingDevice"
  | "profile.missingFields"
  | "profile.missingHost"
  | "profile.networkKeyPath"
  | "profile.notFound"
  | "profile.usedAsJumpHost"
  | "proxy.authFailed"
  | "proxy.authRequired"
  | "proxy.authUnsupported"
  | "proxy.commandFailed"
  | "proxy.hostTooLong"
  | "proxy.inUse"
  | "proxy.missingAddress"
  | "proxy.missingCommand"
  | "proxy.notFound"
  | "proxy.protocol"
  | "proxy.refused"
  | "proxy.timeout"
  | "proxy.unreachable"
  | "proxy.unsafeName"
  | "pty.openFailed"
  | "pty.spawnFailed"
  | "serial.disconnected"
  | "serial.openFailed"
  | "session.busy"
  | "session.notConnected"
  | "session.notFound"
  | "session.pageGone"
  | "sftp.chmodFailed"
  | "sftp.listFailed"
  | "sftp.mkdirFailed"
  | "sftp.openFailed"
  | "sftp.removeFailed"
  | "sftp.renameFailed"
  | "sftp.unsupported"
  | "ssh.channelFailed"
  | "ssh.handshakeFailed"
  | "ssh.handshakeTimeout"
  | "ssh.hostKeyRejected"
  | "ssh.jumpFailed"
  | "transfer.cancelled"
  | "transfer.createDirFailed"
  | "transfer.createFailed"
  | "transfer.deleteElsewhere"
  | "transfer.deleteFailed"
  | "transfer.downloadFailed"
  | "transfer.invalidName"
  | "transfer.invalidPath"
  | "transfer.openFailed"
  | "transfer.readFailed"
  | "transfer.replaceFailed"
  | "transfer.replaceInterrupted"
  | "transfer.uploadFailed"
  | "unexpected"
  | "update.failed"
  | "vault.emptyPassword"
  | "vault.keyMismatch"
  | "vault.keyMissing"
  | "vault.locked"
  | "vault.moveAsideFailed"
  | "vault.notLocked"
  | "vault.parseFailed"
  | "vault.passwordRequired"
  | "vault.readFailed"
  | "vault.verifyFailed"
  | "vault.wrongPassword"
  | "window.notFound"
  | "zmodem.cancelled"
  | "zmodem.corrupt"
  | "zmodem.incomplete"
  | "zmodem.invalidName"
  | "zmodem.notFile"
  | "zmodem.notStarted"
  | "zmodem.protocol"
  | "zmodem.remoteCancelled"
  | "zmodem.timeout";

/**
 * How the user unlocks, for the frontend to name it.
 */
export type AppLockMethod = "touchId" | "password" | "windowsHello" | "polkit";

export type AppLockSettings = { enabled: boolean, 
/**
 * Locks after the system (not only ZShell) has had no input for this long; 0 never.
 */
idleMinutes: number, 
/**
 * Locks when the system locks its screen, sleeps or switches to another user.
 */
onSystemLock: boolean, };

export type AppLockStatus = { locked: boolean, 
/**
 * `None` while the vault is locked.
 */
settings: AppLockSettings | null, 
/**
 * `None` when the system offers no way to authenticate, which `unavailable` explains.
 */
method: AppLockMethod | null, unavailable: CommandError | null, 
/**
 * Whether the system's idle time can be known, and its locking and sleeping.
 */
idle: boolean, systemLock: boolean, };

export type Appearance = "system" | "dark" | "light";

export type AuthMethod = { "type": "auto" } | { "type": "password" } | { "type": "publicKey", keyPath: string, } | { "type": "agent" };

/**
 * What a window saved that the other windows show, which they read again when they hear of
 * it (the `config-changed` event).
 */
export type Change = "settings" | "profiles" | "quickCommands" | "vault" | "appLock";

/**
 * A channel on an SSH connection: the number this side gave it.
 */
export type ChannelInfo = { connection: ConnectionInfo, channel: number, };

export type Cjk = "zh-Hans" | "zh-Hant" | "ja" | "ko";

export type CloseReason = "exited" | "lost" | "failed";

/**
 * An error returned by a backend command.
 */
export type CommandError = { code: ErrorCode, params: { [key in string]: string }, 
/**
 * Already localized by the backend.
 */
message: string, };

export type CommandGroup = { id: string, name: string, commands: Array<QuickCommand>, };

/**
 * What a session connects to, with the settings of its protocol.
 */
export type Connection = { "protocol": "ssh" } & SshOptions | { "protocol": "telnet" } & Remote | { "protocol": "serial" } & SerialOptions;

/**
 * An SSH connection, as the frontend names it next to a count: a number given in the order
 * connections are made since ZShell started, and the port of its TCP connection on this
 * computer (to the first jump host or the proxy, if it goes through one), unless it runs
 * through a proxy command.
 */
export type ConnectionInfo = { id: number, localPort: number | null, };

/**
 * What a meter has counted so far.
 */
export type Counts = { sent: number, received: number, 
/**
 * When something was last sent and received, in milliseconds since the Unix epoch (as
 * JavaScript's `Date.now()`), if it has been.
 */
lastSent: number | null, lastReceived: number | null, };

export type CursorStyle = "block" | "bar" | "underline";

/**
 * A remote item being dragged.
 */
export type DragItem = { path: string, isDir: boolean, 
/**
 * Its permission bits (those of what a link points to), which its placeholder is made
 * with, as a download's copy gets them; unknown for some servers. macOS makes none.
 */
permissions: number | null, };

/**
 * The download of a drop, reported to the frontend as one transfer.
 */
export type DragOutEvent = { "type": "progress" } & TransferProgress | { "type": "done", paths: Array<string>, } | { "type": "error", error: CommandError, };

export type DragOutcome = "cancelled" | "inside" | "outside";

/**
 * How a drag ended, with where the mouse was released (in the page's CSS pixels) for drops
 * on this window.
 */
export type DragResult = { outcome: DragOutcome, x: number, y: number, };

/**
 * Where a dragged tab would go if released now.
 */
export type DropTarget = { "kind": "here" } | { "kind": "window", label: string, x: number, } | { "kind": "elsewhere", cursor: Point | null, } | { "kind": "native" };

/**
 * A tab dropped outside the windows: the cursor in screen coordinates, if known (from
 * [`DropTarget`]), and where it held the tab in the window it came from, in CSS pixels from
 * its top left.
 */
export type Dropped = { cursor: Point | null, grab: Point, };

/**
 * What the watcher sends the frontend.
 */
export type EditEvent = { "type": "changed" };

export type EnvVar = { name: string, value: string, };

export type FileEntry = { name: string, path: string, 
/**
 * True for directories and for symlinks pointing at directories.
 */
isDir: boolean, isSymlink: boolean, size: number, 
/**
 * Seconds since the Unix epoch.
 */
modified: number | null, 
/**
 * Of the entry itself: for a symlink, its own (usually `0777` on Linux).
 */
permissions: number | null, 
/**
 * For a symlink, those of what it points to, which changing its permissions changes
 * (SFTP's `SETSTAT` follows links); unknown for a broken one.
 */
targetPermissions?: number, };

/**
 * Remote files: SFTP and ZMODEM downloads, and editing remote files locally.
 */
export type FileSettings = { 
/**
 * Where downloads go without asking; empty for the Downloads folder.
 */
downloadDirectory: string, 
/**
 * The application remote files are edited with (a `.app` on macOS, an executable on
 * Windows); empty for the one the system opens the file type with.
 */
editor: string, };

export type FlowControl = "none" | "software" | "hardware";

export type Folder = { 
/**
 * Empty when creating a new folder; assigned on save.
 */
id: string, name: string, 
/**
 * The folder this one is in; `None` for the top level.
 */
parent?: string, };

export type ForwardKind = "local" | "remote" | "dynamic";

/**
 * A saved forwarding rule. `target_*` is unused for dynamic rules.
 */
export type ForwardRule = { 
/**
 * Empty for a new rule; assigned on save.
 */
id: string, kind: ForwardKind, 
/**
 * Loopback when empty (filled in on save); `*` ([`ALL_ADDRESSES`]) is every address.
 */
bindHost: string, 
/**
 * 0 lets the OS (or the server, for remote rules) pick a port.
 */
bindPort: number, targetHost: string, targetPort: number, description: string, autoStart: boolean, };

/**
 * The state of a port forwarding rule, as reported to the tabs of its session.
 */
export type ForwardState = { "type": "starting" } | { "type": "active", 
/**
 * The address actually listened on (with the real port when 0 was requested).
 */
bound: string, connections: number, 
/**
 * Why the most recent connection through this rule failed, if one did.
 */
lastError: CommandError | null, } | { "type": "failed", error: CommandError, } | { "type": "stopped" };

/**
 * A forwarding rule's run, for [`SessionEvent::ForwardTraffic`].
 */
export type ForwardTraffic = { counts: Counts, 
/**
 * The connection it runs on.
 */
connection: ConnectionInfo, 
/**
 * The channels of the connections it carries now, lowest first: the first
 * [`FORWARD_CHANNELS_LISTED`] of `channel_count`.
 */
channels: Array<number>, channelCount: number, };

/**
 * A host found in the config, as it would be imported.
 */
export type ImportCandidate = { alias: string, host: string, port: number, username: string, auth: AuthMethod, 
/**
 * `ProxyJump` entries as written: aliases or `[user@]host[:port]`. Also the host of a
 * `ProxyCommand ssh -W %h:%p host`, the older way to write it.
 */
jumpHosts: Array<string>, 
/**
 * Any other `ProxyCommand`, as written.
 */
proxyCommand: string | null, keepaliveInterval: number, forwards: Array<ForwardRule>, forwardAgent: boolean, 
/**
 * From `SetEnv`.
 */
env: Array<EnvVar>, 
/**
 * The name of an existing profile for the same alias or address; such hosts are not
 * imported again.
 */
existing: string | null, 
/**
 * Options in the config that are not imported.
 */
skipped: Array<string>, };

/**
 * When the system is kept from sleeping while sessions are connected.
 */
export type KeepAwakeMode = "off" | "pluggedIn" | "always";

/**
 * A line of the file with a host key.
 */
export type KnownHost = { 
/**
 * 1-based, counting every line of the file.
 */
line: number, 
/**
 * The line as written (without its line ending), which identifies it for removal.
 */
text: string, 
/**
 * `cert-authority` or `revoked`, from `@cert-authority` / `@revoked`.
 */
marker: string | null, 
/**
 * Host names or patterns; hashed ones as written (`|1|salt|hash`).
 */
hosts: Array<string>, 
/**
 * The key type as written, e.g. `ssh-ed25519`.
 */
algorithm: string, 
/**
 * `SHA256:…`, as OpenSSH shows it; none when the key can't be read.
 */
fingerprint: string | null, comment: string | null, };

/**
 * The substitutions of a terminal's font (see the module).
 */
export type LigatureRules = { 
/**
 * The family found, one of those asked for.
 */
family: string, 
/**
 * The number of glyphs the rules tell apart, which also stands for any other.
 */
glyphCount: number, 
/**
 * The glyphs of characters, by runs of characters in triples: the first, the last, and the
 * first's glyph, which the others' follow on from (unless it is `glyph_count`, the glyph of
 * all). Only where a glyph the rules tell apart, or `glyph_count` for `missing_glyph`, is.
 */
chars: Array<number>, 
/**
 * The glyph of characters not in `chars` (that of glyph 0, which characters the font lacks get).
 */
missingGlyph: number, 
/**
 * HarfBuzz's properties of each glyph (and of `glyph_count`), as far as lookup flags look
 * at them; empty if none does.
 */
props: Array<number>, 
/**
 * Where each lookup starts in `data`.
 */
lookups: Array<number>, 
/**
 * The lookups applied, in order.
 */
applied: Array<number>, data: Array<number>, };

/**
 * Which character sequences the terminal draws together, so that a font with programming
 * ligatures draws them as one (the font still decides how they look).
 */
export type Ligatures = "off" | "rules" | "full";

export type Listing = { 
/**
 * Canonical path of the listed directory.
 */
path: string, 
/**
 * In the server's order: the file panel sorts them as the user chose.
 */
entries: Array<FileEntry>, };

/**
 * The languages in effect, for the frontend.
 */
export type Locales = { 
/**
 * The language of the interface.
 */
current: string, 
/**
 * What the `system` setting chooses.
 */
system: string, 
/**
 * The first CJK language among the system's preferred ones, whose fonts come first when
 * the interface isn't in one: the same code point has different glyphs in Chinese and
 * Japanese fonts.
 */
cjk: Cjk, };

/**
 * Why the vault is locked, for the unlock screen.
 */
export type LockReason = "new" | "saved" | "password" | "keyUnavailable" | "unreadable" | "damaged" | "newer";

export type LogFormat = "text" | "raw";

/**
 * How a new session's log starts, as the frontend asks.
 */
export type LogOpen = { "mode": "auto" } | { "mode": "append", path: string, } | { "mode": "off" };

/**
 * Session logs (see `logging/`).
 */
export type LogSettings = { 
/**
 * Where logs are written; empty for the default (`ZShellLogs` in Documents).
 */
directory: string, 
/**
 * File name, with `{session}`, `{host}`, `{user}`, `{date}` and `{time}` replaced.
 */
fileName: string, format: LogFormat, 
/**
 * Start each line with the time it was written (plain text only).
 */
timestamps: boolean, 
/**
 * How that time is written, as are the times of our own messages in the terminal (see
 * `model/time_format.rs`).
 */
timeFormat: string, 
/**
 * Record local terminals from the start, as sessions can be set to.
 */
autoLocal: boolean, 
/**
 * Delete logs older than this many days; 0 keeps them.
 */
keepDays: number, };

export type LogSummary = { count: number, bytes: number, };

/**
 * The state of a window's maximize button as the system sees it: under the pointer, pressed
 * (Windows, see `win.rs`; the page draws the button).
 */
export type MaximizeButtonState = { hover: boolean, pressed: boolean, };

/**
 * A menu of the menu bar, or a part of the app menu outside macOS.
 */
export type MenuSection = "app" | "file" | "edit" | "view" | "help";

export type NativeMenuItem = { "kind": "separator" } | { "kind": "action", id: string, label: string, keys: string | null, };

/**
 * The page's items for one menu of the menu bar.
 */
export type NativeMenuSection = { section: MenuSection, items: Array<NativeMenuItem>, };

export type Parity = "none" | "odd" | "even";

/**
 * A point: in a page's CSS pixels, or in screen coordinates (see [`screen`]).
 */
export type Point = { x: number, y: number, };

export type PowerSettings = { 
/**
 * Keep the system from sleeping while idle as long as sessions are connected (see `keep_awake`).
 */
keepAwake: KeepAwakeMode, };

/**
 * A saved session: what it connects to (by protocol), and how its tabs behave.
 */
export type Profile = { 
/**
 * Empty when creating a new profile; assigned on save.
 */
id: string, name: string, 
/**
 * The folder the session is in; `None` for the top level. Changed with
 * [`ProfileStore::move_item`]; `save` keeps it.
 */
folder?: string, connection: Connection, 
/**
 * Reconnect automatically when an established connection is lost, or when a serial
 * device is plugged in again.
 */
autoReconnect: boolean, 
/**
 * The remote side's character encoding, one of [`crate::encoding::SUPPORTED`].
 */
encoding: string, 
/**
 * Commands typed into each new shell, in order, each once the shell shows a prompt.
 * Sent by the frontend.
 */
loginCommands: Array<string>, 
/**
 * Record a session log from the start of each connection.
 */
autoLog: boolean, 
/**
 * The quick command group its tabs show first; `None` for the default group.
 */
commandGroup?: string, 
/**
 * Terminal appearance for this session; unset values follow the settings.
 */
appearance?: ProfileAppearance, 
/**
 * What the user notes about the session (what the machine is for, how to get root);
 * shown with it in the sidebar and found by the search.
 */
notes?: string, };

/**
 * Overrides of the terminal settings for one session. Only the frontend interprets them.
 */
export type ProfileAppearance = { 
/**
 * A built-in color scheme id, or "auto", as in the settings.
 */
colorScheme?: string, 
/**
 * Replaces the color scheme's background, as `#rrggbb` (a red one for production).
 */
background?: string, fontFamily?: string, fontSize?: number, };

export type Protection = "none" | "system" | "password";

/**
 * What the protection changes to (see `Vault::set_protection`). Shown by `{:?}` without the
 * master password.
 */
export type ProtectionChange = { "protection": "none" } | { "protection": "system" } | { "protection": "password", password: string | null, remember: boolean, };

export type Protocol = "ssh" | "telnet" | "serial";

export type Proxy = { 
/**
 * Empty when creating a new proxy; assigned on save.
 */
id: string, name: string, kind: ProxyKind, 
/**
 * SOCKS5 and HTTP: the proxy server. Kept, unused, by command proxies.
 */
host: string, port: number, 
/**
 * SOCKS5 and HTTP: empty for a proxy without authentication. The password is kept with
 * the others (see `vault/`).
 */
username: string, 
/**
 * Command proxies: run by the user's shell, with `%h`, `%p`, `%r` and `%%` replaced.
 */
command: string, };

export type ProxyKind = "socks5" | "http" | "command";

export type QuickCommand = { id: string, name: string, 
/**
 * Sent as typed; line breaks are Enter.
 */
text: string, 
/**
 * Press Enter after the text; otherwise it is left on the command line to be finished.
 */
enter: boolean, };

export type QuickCommands = { groups: Array<CommandGroup>, };

/**
 * A rectangle in a page's CSS pixels.
 */
export type Rect = { x: number, y: number, width: number, height: number, };

/**
 * A published release.
 */
export type Release = { 
/**
 * Without the tag's `v`: `2.11.0`.
 */
version: string, 
/**
 * Its page on GitHub, with the release notes and the downloads.
 */
url: string, };

/**
 * What SSH and Telnet sessions have in common: where they connect, through what, and the
 * terminal type the remote side is told.
 */
export type Remote = { host: string, port: number, 
/**
 * Required for SSH; for Telnet, optional and typed at the login prompt.
 */
username: string, 
/**
 * Sessions to connect through, in order (OpenSSH's `ProxyJump a,b`). Each hop uses its
 * own session's address and authentication, but not that session's jump hosts. Only
 * SSH sessions can be jump hosts.
 */
jumpHosts: Array<string>, 
/**
 * The id of the proxy to connect through. Not kept with jump hosts, where the first jump
 * host's own proxy is used (see [`Route`]).
 */
proxy?: string, 
/**
 * Seconds between keepalive messages; 0 disables them. Three unanswered ones in a row
 * drop an SSH connection; Telnet uses TCP keepalives, as does SSH until authenticated.
 */
keepaliveInterval: number, 
/**
 * The terminal type the remote side is told (`TERM`, Telnet's terminal type option).
 */
termType: string, };

/**
 * What right-clicking the terminal does.
 */
export type RightClick = "menu" | "paste";

/**
 * A forwarding rule just saved, with its session.
 */
export type SavedForward = { profile: Profile, rule: ForwardRule, };

/**
 * When terminals are read for screen readers.
 */
export type ScreenReaderMode = "auto" | "on" | "off";

/**
 * A serial line's settings: 115200 8N1 without flow control unless set otherwise.
 */
export type SerialOptions = { 
/**
 * `/dev/cu.*` on macOS, `COM3` on Windows.
 */
device: string, baudRate: number, 
/**
 * 5 to 8.
 */
dataBits: number, parity: Parity, 
/**
 * 1 or 2.
 */
stopBits: number, flowControl: FlowControl, };

/**
 * A serial port found on this computer.
 */
export type SerialPortInfo = { 
/**
 * What to open: `/dev/cu.usbserial-1410`, `COM3`.
 */
name: string, 
/**
 * The USB adapter's product or manufacturer name, if known.
 */
description: string | null, };

/**
 * A session in a file, as it would be imported.
 */
export type SessionCandidate = { id: string, name: string, 
/**
 * As in the file: its jump hosts and proxy are ids in the file.
 */
connection: Connection, 
/**
 * The names of the folders it is in, outermost first.
 */
folder: Array<string>, 
/**
 * The names of its jump hosts.
 */
jumpHosts: Array<string>, 
/**
 * The name of its proxy: one that exists here if importing uses it instead (the same
 * name and kind, or the same server or command).
 */
proxy: string | null, 
/**
 * Whether `proxy` is one that exists here.
 */
existingProxy: boolean, 
/**
 * The command its proxy runs, for a command proxy imported with it: importing it means
 * that this command runs when the session connects.
 */
proxyCommand: string | null, 
/**
 * The name of an existing session with the same name or address; such sessions are not
 * imported again.
 */
existing: string | null, 
/**
 * Forwarding rules that start when it connects, as they will be saved: importing them
 * means listening on those ports.
 */
autoForwards: Array<ForwardRule>, 
/**
 * Commands typed into its shell once it connects: importing them means running them on
 * the server, which the file may name as one of the user's own.
 */
loginCommands: Array<string>, 
/**
 * The other sessions in the file (ids) that importing it imports too: the jump hosts it
 * goes through, and theirs, that don't exist here. Their proxies and forwarding rules
 * come with them; with jump hosts, the first one's proxy is the one used.
 */
brings: Array<string>, };

export type SessionEvent = { "type": "connected" } | { "type": "closed", reason: CloseReason, error: CommandError | null, status: number | null, signal: string | null, } | { "type": "forward", ruleId: string, state: ForwardState, } | { "type": "zmodem", phase: ZmodemPhase, } | { "type": "log", path: string | null, error: CommandError | null, } | { "type": "waiting", connections: number, } | { "type": "terminalTraffic", counts: Counts, channel: ChannelInfo | null, } | { "type": "sftpTraffic", connection: ConnectionInfo, browse: SftpChannelTraffic, transfer: SftpChannelTraffic, } | { "type": "forwardTraffic", ruleId: string, traffic: ForwardTraffic, } | { "type": "moved", moveId: string, };

/**
 * What a session's terminal showed in the tab it moved from: written to the new tab's
 * terminal before the output that came since.
 */
export type SessionSnapshot = { id: number, 
/**
 * The move it was taken for (see [`SessionEvent::Moved`]).
 */
moveId: string, 
/**
 * The screen and the scrollback, as escape sequences that draw them again.
 */
data: string, 
/**
 * The size of the terminal it was taken from, which it is written into before fitting.
 */
cols: number, rows: number, };

/**
 * What a new session runs.
 */
export type SessionSpec = { "kind": "profile", profileId: string, carry: Array<string>, } | { "kind": "quick", protocol: Protocol, username: string, host: string, port: number, } | { "kind": "shared", source: number, } | { "kind": "local" };

/**
 * What a sessions file holds, as it would be imported, and a digest of the file: importing
 * checks that the file is still what was shown.
 */
export type SessionsScan = { candidates: Array<SessionCandidate>, digest: string, };

export type SetAsideFile = { path: string, 
/**
 * Where the file was moved; `None` if it was left as it is (see [`Found::LeftAlone`]).
 */
movedTo: string | null, error: string, };

export type Settings = { 
/**
 * A language there is a catalog for, or "system" for the system's preferred languages
 * (see `i18n.rs`). A string, so that an unknown one (from a newer version) falls back
 * rather than making the whole file unreadable.
 */
language: string, appearance: Appearance, textSize: TextSize, 
/**
 * The interface's font, a CSS font family list put before the default ones; empty for
 * the default (the terminal's is `terminal.font_family`).
 */
appFont: string, statusBar: StatusBarSettings, terminal: TerminalSettings, tabs: TabSettings, sidebar: SidebarSettings, power: PowerSettings, files: FileSettings, zmodem: ZmodemSettings, logs: LogSettings, updates: UpdateSettings, };

/**
 * One of a connection's SFTP channels (browsing, or transfers), for [`SessionEvent::SftpTraffic`].
 */
export type SftpChannelTraffic = { counts: Counts, 
/**
 * The channel it is on now, if it has been opened. Transfers share the browsing channel
 * when the server refuses them one of their own, and then have its counts too.
 */
channel: number | null, };

export type SidebarSettings = { 
/**
 * Show the most recently opened sessions above the list.
 */
showRecent: boolean, };

export type SshOptions = { auth: AuthMethod, 
/**
 * Let the remote shell use the local SSH agent (OpenSSH's `ForwardAgent`).
 */
forwardAgent: boolean, 
/**
 * Environment variables for the remote shell (OpenSSH's `SetEnv`); the server only
 * accepts those its `AcceptEnv` allows.
 */
env: Array<EnvVar>, 
/**
 * Edited separately, one at a time (`ProfileStore::save_forward`); `save` keeps them.
 */
forwards: Array<ForwardRule>, host: string, port: number, 
/**
 * Required for SSH; for Telnet, optional and typed at the login prompt.
 */
username: string, 
/**
 * Sessions to connect through, in order (OpenSSH's `ProxyJump a,b`). Each hop uses its
 * own session's address and authentication, but not that session's jump hosts. Only
 * SSH sessions can be jump hosts.
 */
jumpHosts: Array<string>, 
/**
 * The id of the proxy to connect through. Not kept with jump hosts, where the first jump
 * host's own proxy is used (see [`Route`]).
 */
proxy?: string, 
/**
 * Seconds between keepalive messages; 0 disables them. Three unanswered ones in a row
 * drop an SSH connection; Telnet uses TCP keepalives, as does SSH until authenticated.
 */
keepaliveInterval: number, 
/**
 * The terminal type the remote side is told (`TERM`, Telnet's terminal type option).
 */
termType: string, };

/**
 * The bar under the terminals with the focused pane's connection, activity, terminal type,
 * size and cursor position.
 */
export type StatusBarSettings = { show: boolean, };

/**
 * Which system store the platform has, for the frontend to name it.
 */
export type StoreKind = "keychain" | "credentialManager" | "secretService";

/**
 * The end of a system drag of a tab (`tab-drag-native-end`, to the window it came from).
 */
export type TabDragNativeEnd = { id: string, dropped: boolean, };

export type TabSettings = { 
/**
 * Show the title set by the shell (OSC 0 / 2) instead of the session name.
 */
followRemoteTitle: boolean, 
/**
 * Ask before closing tabs that are connected or running a program.
 */
confirmClose: boolean, 
/**
 * Mark the tabs in the background that have new output, until they are shown.
 */
markOutput: boolean, };

/**
 * A program's progress, or a window's: the merge of its tabs'.
 */
export type TaskProgress = { state: TaskState, 
/**
 * From 0 to 100; not shown while indeterminate.
 */
percent: number, };

/**
 * The states of OSC 9;4 (besides 0, which removes the progress).
 */
export type TaskState = "normal" | "error" | "indeterminate" | "paused";

export type TerminalSettings = { 
/**
 * A built-in color scheme id, or "auto" to follow the appearance.
 */
colorScheme: string, 
/**
 * Preferred font family; empty uses the platform default. Fallbacks (including CJK
 * fonts) are always appended by the frontend.
 */
fontFamily: string, fontSize: number, ligatures: Ligatures, cursorStyle: CursorStyle, cursorBlink: boolean, 
/**
 * Lines kept above the screen.
 */
scrollback: number, 
/**
 * Copy text to the clipboard as soon as it is selected.
 */
copyOnSelect: boolean, rightClick: RightClick, 
/**
 * Ask before pasting text with line breaks while the shell would run each line.
 */
confirmMultilinePaste: boolean, 
/**
 * macOS: the Option key sends Meta (Esc-prefixed) sequences instead of special characters.
 */
optionAsMeta: boolean, 
/**
 * Programs in the terminal can copy text to the clipboard (OSC 52); they can never read it.
 */
clipboardWrite: boolean, 
/**
 * The most text they copy at once, in KiB; more is ignored.
 */
clipboardLimit: number, 
/**
 * Programs in the terminal can notify the user (OSC 9, 777 and 99) when it is out of sight.
 */
notifications: boolean, 
/**
 * Whether screen readers can read the terminal and hear new output (xterm.js's
 * `screenReaderMode`), which slows down terminals with much output: by default while the
 * system's screen reader runs.
 */
screenReaderMode: ScreenReaderMode, 
/**
 * A bell (BEL) not caused by typing marks its tab if in the background, tells screen
 * readers, and asks for attention while the window is in the background.
 */
bellAlert: boolean, 
/**
 * A bell plays the system's alert sound.
 */
bellSound: boolean, 
/**
 * A bell flashes its terminal (a visual bell).
 */
bellFlash: boolean, };

/**
 * The size of the interface's text (not the terminal's, which has its own font size).
 */
export type TextSize = "normal" | "large" | "larger";

/**
 * A part of a parsed time format.
 */
export type TimePart = { "type": "text", text: string, } | { "type": "year" } | { "type": "shortYear" } | { "type": "month" } | { "type": "day" } | { "type": "hour" } | { "type": "minute" } | { "type": "second" } | { "type": "millisecond" };

/**
 * What deleting what a transfer left did, for the file panel.
 */
export type TransferDeletion = { 
/**
 * How many files and folders it deleted.
 */
deleted: number, deletedFolders: number, 
/**
 * How many files it kept, changed since the transfer.
 */
changed: number, 
/**
 * How many files and folders are still to delete, after a failure (as in [`Ended`]).
 */
created: number, folders: number, 
/**
 * Why it stopped; what is still to delete can be deleted again.
 */
error: CommandError | null, };

/**
 * How a transfer ended, for the file panel.
 */
export type TransferEnd = { 
/**
 * The local items it left, to show (downloads).
 */
paths: Array<string>, 
/**
 * How many files and folders it created, which `transfer_delete` deletes (the folders
 * once nothing else is in them).
 */
created: number, folders: number, 
/**
 * How many files it wrote over, which it doesn't.
 */
replaced: number, 
/**
 * Why it stopped, if it didn't finish.
 */
error: CommandError | null, };

export type TransferProgress = { transferred: number, total: number, filesDone: number, filesTotal: number, current: string, };

/**
 * A session or a folder: as dragged in the sidebar, and in the order they are shown.
 */
export type TreeItem = { "kind": "profile", id: string, } | { "kind": "folder", id: string, };

/**
 * Checking for a newer version (`update.rs`).
 */
export type UpdateSettings = { 
/**
 * Once a day, automatically.
 */
check: boolean, 
/**
 * A version the user doesn't want to hear about again.
 */
skipped: string | null, };

/**
 * What the frontend shows.
 */
export type UpdateStatus = { 
/**
 * False in the Microsoft Store package, which never checks.
 */
supported: boolean, 
/**
 * The latest release, when it is newer than this version.
 */
newer: Release | null, };

export type VaultLock = { reason: LockReason, 
/**
 * What went wrong, if something did (reading the key, the file).
 */
error: CommandError | null, };

export type VaultStatus = { 
/**
 * `None` when locked with no vault, or one whose header can't be read.
 */
protection: Protection | null, 
/**
 * With password protection: the data key is also in the credential store.
 */
remember: boolean, locked: VaultLock | null, store: StoreKind, 
/**
 * Set by the [`Vault::try_unlock`] that made the vault, which chose its protection
 * without asking: the frontend says how it is protected, and offers to change it, once.
 */
created: boolean, };

/**
 * A window button the frontend draws.
 */
export type WindowButton = "minimize" | "maximize" | "close";

/**
 * What a window is to show: a tab moved out of another window (dropped `at` that many CSS
 * pixels from the left of the page, if dragged there), or a saved session to open. The tab
 * is the frontend's (its panes, their layout and state), passed on without being read.
 */
export type WindowContent = { "kind": "tab", tab: unknown, at: number | null, } | { "kind": "profile", profileId: string, title: string, };

/**
 * Another window, as the tab menu lists it.
 */
export type WindowInfo = { label: string, title: string, };

/**
 * What the frontend is asked for, or that a transfer is running (for its cancel button).
 */
export type ZmodemPhase = "chooseDestination" | "chooseFiles" | "transferring" | "idle";

/**
 * What happens when `sz` sends files. Asked by default: a file that looks like `sz` output
 * (`cat` of one) starts a download, which shouldn't happen without the user.
 */
export type ZmodemReceive = "ask" | "downloads" | "chooseFolder";

export type ZmodemSettings = { 
/**
 * What happens when `sz` sends files.
 */
receive: ZmodemReceive, };

/** The events the backend sends the pages, by name, with their payloads (see `events.rs`). */
export type Events = {
  "app-locked": null;
  "busy-query": number;
  "config-changed": Change;
  "menu-action": string;
  "offers-withdrawn": Array<number>;
  "pane-stayed": number;
  "quit-requested": null;
  "screen-reader-changed": boolean;
  "session-snapshot": SessionSnapshot;
  "tab-drag-native-end": TabDragNativeEnd;
  "tab-drag-over": number | null;
  "tab-received": null;
  "update-status": UpdateStatus;
  "window-buttons-changed": null;
  "window-closed": string;
  "window-maximize-button": MaximizeButtonState;
  "windows-changed": null;
};

/** The commands the frontend can call (see `commands/`): the arguments each takes, and what it returns. */
export type Commands = {
  app_bell: { args: Record<string, never>; result: void };
  app_busy_count: { args: Record<string, never>; result: number };
  app_lock_configure: { args: { settings: AppLockSettings }; result: AppLockStatus };
  app_lock_lock: { args: Record<string, never>; result: AppLockStatus };
  app_lock_status: { args: { method: boolean }; result: AppLockStatus };
  app_lock_unlock: { args: { anyway: boolean }; result: AppLockStatus };
  app_menu_set: { args: { sections: NativeMenuSection[] }; result: void };
  app_menu_set_label: { args: { id: string; label: string }; result: void };
  app_quit: { args: Record<string, never>; result: void };
  color_pick: { args: Record<string, never>; result: string | null };
  config_directory: { args: Record<string, never>; result: string };
  config_set_aside: { args: Record<string, never>; result: SetAsideFile[] };
  credential_store_available: { args: Record<string, never>; result: boolean };
  downloads_directory: { args: Record<string, never>; result: string };
  folder_delete: { args: { id: string }; result: void };
  folder_save: { args: { folder: Folder }; result: Folder };
  folders_list: { args: Record<string, never>; result: Folder[] };
  forward_carry: { args: { id: number }; result: string[] };
  forward_connections: { args: { ids: number[] }; result: number };
  forward_keep: { args: { ids: number[] }; result: void };
  forward_keep_candidates: { args: { ids: number[] }; result: ForwardRule[] };
  forward_sessions_of: { args: { profileId: string }; result: number[] };
  forward_start: { args: { id: number; rule: ForwardRule }; result: void };
  forward_stop: { args: { id: number; ruleId: string }; result: void };
  home_directory: { args: Record<string, never>; result: string | null };
  keep_awake_limited: { args: Record<string, never>; result: boolean };
  known_hosts_find: { args: { query: string }; result: number[] };
  known_hosts_list: { args: Record<string, never>; result: KnownHost[] };
  known_hosts_path: { args: Record<string, never>; result: string | null };
  known_hosts_remove: { args: { line: number; text: string }; result: void };
  local_shell_name: { args: Record<string, never>; result: string | null };
  local_username: { args: Record<string, never>; result: string };
  locale_get: { args: Record<string, never>; result: Locales };
  logs_count: { args: { profileId: string }; result: number };
  logs_delete: { args: { profileId?: string | null }; result: number };
  logs_directory: { args: { create: boolean }; result: string };
  logs_summary: { args: Record<string, never>; result: LogSummary };
  logs_time_format: { args: { format: string }; result: TimePart[] };
  profile_delete: { args: { id: string }; result: void };
  profile_duplicate: { args: { id: string; name: string }; result: Profile };
  profile_forward_delete: { args: { profileId: string; ruleId: string }; result: Profile };
  profile_forward_save: { args: { profileId: string; rule: ForwardRule }; result: SavedForward };
  profile_save: { args: { profile: Profile; password?: string | null }; result: Profile };
  profiles_list: { args: Record<string, never>; result: Profile[] };
  proxies_list: { args: Record<string, never>; result: Proxy[] };
  proxy_delete: { args: { id: string }; result: void };
  proxy_save: { args: { proxy: Proxy; password?: string | null }; result: Proxy };
  quick_commands_get: { args: Record<string, never>; result: QuickCommands };
  quick_commands_set: { args: { commands: QuickCommands }; result: QuickCommands };
  screen_reader_running: { args: Record<string, never>; result: boolean };
  serial_ports: { args: Record<string, never>; result: SerialPortInfo[] };
  session_ack: { args: { id: number; bytes: number }; result: void };
  session_adopt: { args: { id: number; channel: Channel<ArrayBuffer>; moveId: string }; result: void };
  session_break: { args: { id: number }; result: void };
  session_close: { args: { id: number }; result: void };
  session_foreground: { args: { id: number }; result: string | null };
  session_handover: { args: { snapshot: SessionSnapshot }; result: void };
  session_log_start: { args: { id: number }; result: string };
  session_log_stop: { args: { id: number }; result: void };
  session_open: { args: { spec: SessionSpec; cols: number; rows: number; log?: LogOpen | null; channel: Channel<ArrayBuffer> }; result: number };
  session_resize: { args: { id: number; cols: number; rows: number }; result: void };
  session_write: { args: { id: number; data: string }; result: void };
  session_write_bytes: { args: { id: number; data: number[] }; result: void };
  sessions_export: { args: { path: string; passphrase?: string | null }; result: void };
  sessions_import: { args: { path: string; ids: string[]; digest: string; passphrase?: string | null }; result: void };
  sessions_import_scan: { args: { path: string; passphrase?: string | null }; result: SessionsScan };
  settings_get: { args: Record<string, never>; result: Settings };
  settings_set: { args: { settings: Settings }; result: Settings };
  sftp_chmod: { args: { id: number; path: string; mode: number }; result: void };
  sftp_download: { args: { id: number; transferId: string; remotePaths: string[]; localDir?: string | null; onProgress: Channel<TransferProgress> }; result: TransferEnd };
  sftp_download_as: { args: { id: number; transferId: string; remotePath: string; localPath: string; onProgress: Channel<TransferProgress> }; result: TransferEnd };
  sftp_drag_out: { args: { id: number; transferId: string; items: DragItem[]; sincePress: number; onEvent: Channel<DragOutEvent> }; result: DragResult };
  sftp_edit_open: { args: { id: number; editId: string; remotePath: string; onProgress: Channel<TransferProgress>; onEvent: Channel<EditEvent> }; result: string };
  sftp_edit_reopen: { args: { editId: string }; result: void };
  sftp_edit_stop: { args: { editId: string }; result: void };
  sftp_edit_upload: { args: { id: number; editId: string; force: boolean }; result: void };
  sftp_list: { args: { id: number; path: string }; result: Listing };
  sftp_mkdir: { args: { id: number; path: string }; result: void };
  sftp_open: { args: { id: number }; result: string };
  sftp_remove: { args: { id: number; path: string }; result: void };
  sftp_rename: { args: { id: number; from: string; to: string }; result: void };
  sftp_shell_dir: { args: { id: number }; result: string | null };
  sftp_upload: { args: { id: number; transferId: string; localPaths: string[]; remoteDir: string; onProgress: Channel<TransferProgress> }; result: TransferEnd };
  ssh_config_default_path: { args: Record<string, never>; result: string | null };
  ssh_config_import: { args: { path: string; aliases: string[] }; result: Profile[] };
  ssh_config_scan: { args: { path: string }; result: ImportCandidate[] };
  ssh_default_key_path: { args: Record<string, never>; result: string | null };
  ssh_key_file_exists: { args: { path: string }; result: boolean };
  system_monospace_font: { args: Record<string, never>; result: string | null };
  tab_drag_end: { args: { id: string; dropped: boolean; outside: boolean }; result: DropTarget | null };
  tab_drag_move: { args: { id: string; outside: boolean }; result: DropTarget };
  terminal_ligature_rules: { args: { families: string[] }; result: LigatureRules | null };
  transfer_cancel: { args: { transferId: string }; result: void };
  transfer_delete: { args: { id?: number | null; transferId: string }; result: TransferDeletion };
  transfer_forget: { args: { transferIds: string[] }; result: void };
  tree_move: { args: { item: TreeItem; parent?: string | null; before?: TreeItem | null }; result: void };
  tree_order: { args: Record<string, never>; result: TreeItem[] };
  update_check: { args: Record<string, never>; result: UpdateStatus };
  update_status: { args: Record<string, never>; result: UpdateStatus };
  vault_reset: { args: Record<string, never>; result: VaultStatus };
  vault_set_protection: { args: { change: ProtectionChange; current?: string | null }; result: VaultStatus };
  vault_status: { args: Record<string, never>; result: VaultStatus };
  vault_try_unlock: { args: Record<string, never>; result: VaultStatus };
  vault_unlock: { args: { password: string }; result: VaultStatus };
  window_busy_reply: { args: { query: number; count: number }; result: void };
  window_buttons: { args: Record<string, never>; result: WindowButton[] };
  window_close_empty: { args: Record<string, never>; result: boolean };
  window_file_dialog: { args: { open: boolean }; result: void };
  window_list: { args: Record<string, never>; result: WindowInfo[] };
  window_open: { args: { content: WindowContent; sessions: number[]; dropped?: Dropped | null }; result: string };
  window_pane_stayed: { args: { target: string; pane: number }; result: void };
  window_send_tab: { args: { target: string; tab: unknown; sessions: number[]; at?: number | null }; result: void };
  window_set_connected: { args: { connected: boolean }; result: void };
  window_set_drag_area: { args: { rect?: Rect | null }; result: void };
  window_set_maximize_button: { args: { rect?: Rect | null }; result: void };
  window_set_progress: { args: { progress?: TaskProgress | null }; result: void };
  window_set_snapped_border_color: { args: { color: string }; result: void };
  window_set_tab_bar: { args: { rect: Rect }; result: void };
  window_set_theme: { args: { theme?: "light" | "dark" | null }; result: void };
  window_set_title: { args: { title: string }; result: void };
  window_system_menu: { args: Record<string, never>; result: void };
  window_take: { args: Record<string, never>; result: WindowContent[] };
  window_title_double_click: { args: Record<string, never>; result: void };
  zmodem_cancel: { args: { id: number }; result: void };
  zmodem_save_to: { args: { id: number; dir?: string | null }; result: void };
  zmodem_send_files: { args: { id: number; paths: string[] }; result: void };
};

export type Command = keyof Commands;

/** The character encodings a session can use (WHATWG labels, as profiles store them). */
export const ENCODINGS = ["utf-8", "gb18030", "gbk", "big5", "shift_jis", "euc-jp", "euc-kr", "windows-1252"] as const;

/** The terminal type of a session that doesn't set one. */
export const DEFAULT_TERM_TYPE = "xterm-256color";

/** The settings of a new serial session. */
export const DEFAULT_SERIAL: SerialOptions = { baudRate: 115200, dataBits: 8, device: "", flowControl: "none", parity: "none", stopBits: 1 };

/** The quick command group that always exists, first. */
export const DEFAULT_GROUP = "default";

/** The languages with a catalog, English first. */
export const LANGUAGES = ["en", "zh-Hans", "ja", "de", "fr"] as const;

/** The settings before any change, which "Restore Defaults" goes back to. */
export const DEFAULT_SETTINGS: Settings = { appFont: "", appearance: "system", files: { downloadDirectory: "", editor: "" }, language: "system", logs: { autoLocal: false, directory: "", fileName: "{session}_{date}_{time}.log", format: "text", keepDays: 0, timeFormat: "[%Y-%m-%d %H:%M:%S]", timestamps: false }, power: { keepAwake: "off" }, sidebar: { showRecent: true }, statusBar: { show: true }, tabs: { confirmClose: true, followRemoteTitle: true, markOutput: true }, terminal: { bellAlert: true, bellFlash: false, bellSound: true, clipboardLimit: 1024, clipboardWrite: true, colorScheme: "auto", confirmMultilinePaste: true, copyOnSelect: false, cursorBlink: true, cursorStyle: "block", fontFamily: "", fontSize: 13, ligatures: "off", notifications: true, optionAsMeta: false, rightClick: "menu", screenReaderMode: "auto", scrollback: 5000 }, textSize: "normal", updates: { check: true, skipped: null }, zmodem: { receive: "ask" } };

/** The parts of the default time format of logs and of our own messages in the terminal. */
export const DEFAULT_TIME_FORMAT_PARTS: TimePart[] = [{ text: "[", type: "text" }, { type: "year" }, { text: "-", type: "text" }, { type: "month" }, { text: "-", type: "text" }, { type: "day" }, { text: " ", type: "text" }, { type: "hour" }, { text: ":", type: "text" }, { type: "minute" }, { text: ":", type: "text" }, { type: "second" }, { text: "]", type: "text" }];

/** The smallest terminal font size, in the settings and in sessions. */
export const FONT_SIZE_MIN = 6;

/** The largest terminal font size, in the settings and in sessions. */
export const FONT_SIZE_MAX = 48;

/** The most lines a terminal keeps above the screen. */
export const SCROLLBACK_MAX = 100000;

/** The most a program can copy to the clipboard at once, in KiB. */
export const CLIPBOARD_LIMIT_MAX = 7168;

/** The kinds of subtables in a font's ligature rules (see `fonts/rules.rs`). */
export const LIGATURE_SUBTABLES = { classRules: 5, coverageRules: 6, glyphRules: 4, ligature: 3, multiple: 2, reverse: 7, single: 1 } as const;

/** No table, glyph or lookup in a font's ligature rules. */
export const LIGATURE_NONE = 4294967295;

/** How many of a forwarding rule's channels its traffic reports list. */
export const FORWARD_CHANNELS_LISTED = 20;

/** Processed terminal output is acknowledged in batches of this many bytes (see `session/flow.rs`). */
export const ACK_BATCH = 262144;
