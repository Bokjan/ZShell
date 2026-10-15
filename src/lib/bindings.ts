// Generated from the Rust types and constants by `cargo test` (src-tauri/src/bindings.rs); do not edit.
// After changing them, run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri.

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
  | "import.unsupportedEncryption"
  | "import.unsupportedVersion"
  | "import.wrongPassphrase"
  | "knownHosts.changed"
  | "knownHosts.noHome"
  | "knownHosts.readFailed"
  | "knownHosts.writeFailed"
  | "log.createFailed"
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
  | "vault.deleteFailed"
  | "vault.emptyPassword"
  | "vault.keyMismatch"
  | "vault.keyMissing"
  | "vault.locked"
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

export type CursorStyle = "block" | "bar" | "underline";

/**
 * A remote item being dragged.
 */
export type DragItem = { path: string, isDir: boolean, };

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
export type DropTarget = { "kind": "here" } | { "kind": "window", label: string, x: number, } | { "kind": "elsewhere", cursor: Point, } | { "kind": "native" };

/**
 * A tab dropped outside the windows: the cursor in screen coordinates (from [`DropTarget`]),
 * and where it held the tab in the window it came from, in CSS pixels from its top left.
 */
export type Dropped = { cursor: Point, grab: Point, };

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
id: string, kind: ForwardKind, bindHost: string, 
/**
 * 0 lets the OS (or the server, for remote rules) pick a port.
 */
bindPort: number, targetHost: string, targetPort: number, description: string, autoStart: boolean, };

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

export type Listing = { 
/**
 * Canonical path of the listed directory.
 */
path: string, entries: Array<FileEntry>, };

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
 * Record local terminals from the start, as sessions can be set to.
 */
autoLocal: boolean, 
/**
 * Delete logs older than this many days; 0 keeps them.
 */
keepDays: number, };

export type LogSummary = { count: number, bytes: number, };

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
 * What the protection changes to (see `Vault::set_protection`).
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
 * The name of its proxy.
 */
proxy: string | null, 
/**
 * The command its proxy runs, for a command proxy: importing it means that this
 * command runs when the session connects.
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

export type SessionEvent = { "type": "connected" } | { "type": "closed", reason: CloseReason, error: CommandError | null, status: number | null, signal: string | null, } | { "type": "forward", ruleId: string, state: ForwardState, } | { "type": "zmodem", phase: ZmodemPhase, } | { "type": "log", path: string | null, error: CommandError | null, } | { "type": "moved", moveId: string, };

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
appFont: string, statusBar: StatusBarSettings, terminal: TerminalSettings, tabs: TabSettings, sidebar: SidebarSettings, files: FileSettings, zmodem: ZmodemSettings, logs: LogSettings, updates: UpdateSettings, };

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
 * Edited separately through [`ProfileStore::set_forwards`]; `save` keeps them.
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

export type TerminalSettings = { 
/**
 * A built-in color scheme id, or "auto" to follow the appearance.
 */
colorScheme: string, 
/**
 * Preferred font family; empty uses the platform default. Fallbacks (including CJK
 * fonts) are always appended by the frontend.
 */
fontFamily: string, fontSize: number, cursorStyle: CursorStyle, cursorBlink: boolean, 
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
