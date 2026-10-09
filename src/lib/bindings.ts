// Generated from the Rust types by `cargo test` (src-tauri/src/bindings.rs); do not edit.
// After changing them, run `UPDATE_BINDINGS=1 cargo test bindings` in src-tauri.

/** The `code` of a `CommandError`. */
export type ErrorCode =
  | "auth.agentAllRejected"
  | "auth.agentConnectFailed"
  | "auth.agentEmpty"
  | "auth.agentListFailed"
  | "auth.cancelled"
  | "auth.failed"
  | "auth.keyDecryptFailed"
  | "auth.keyReadFailed"
  | "auth.keyRejected"
  | "auth.passwordFailed"
  | "credentialStore"
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
  | "forward.connectFailed"
  | "forward.connectTimeout"
  | "forward.missingTarget"
  | "forward.remoteRefused"
  | "forward.socksAddressType"
  | "forward.socksAuth"
  | "forward.socksCommand"
  | "forward.socksProtocol"
  | "forward.socksVersion"
  | "import.invalidProxy"
  | "import.invalidSession"
  | "import.notSessionsFile"
  | "import.parseFailed"
  | "import.readFailed"
  | "knownHosts.changed"
  | "knownHosts.noHome"
  | "knownHosts.readFailed"
  | "knownHosts.writeFailed"
  | "log.createFailed"
  | "net.connectFailed"
  | "net.connectTimeout"
  | "net.connectionLost"
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
  | "session.notConnected"
  | "session.notFound"
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
  | "transfer.uploadFailed"
  | "unexpected"
  | "zmodem.cancelled"
  | "zmodem.corrupt"
  | "zmodem.incomplete"
  | "zmodem.invalidName"
  | "zmodem.notFile"
  | "zmodem.notStarted"
  | "zmodem.protocol"
  | "zmodem.remoteCancelled"
  | "zmodem.timeout";

export type Appearance = "system" | "dark" | "light";

export type AuthMethod = { "type": "auto" } | { "type": "password" } | { "type": "publicKey", keyPath: string, } | { "type": "agent" };

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
modified: number | null, permissions: number | null, };

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

export type LogFormat = "text" | "raw";

/**
 * How a new session's log starts, as the frontend asks.
 */
export type LogOpen = { "mode": "auto" } | { "mode": "append", path: string, } | { "mode": "off" };

/**
 * Session logs (see `logging.rs`).
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

export type Parity = "none" | "odd" | "even";

export type Profile = { 
/**
 * Empty when creating a new profile; assigned on save.
 */
id: string, name: string, 
/**
 * Absent in files from before Telnet and serial sessions, which are all SSH.
 */
protocol: Protocol, 
/**
 * SSH and Telnet; unused by serial sessions.
 */
host: string, port: number, 
/**
 * Required for SSH; for Telnet, optional and typed at the login prompt.
 */
username: string, 
/**
 * SSH only. Telnet uses the saved password, if any, at the password prompt.
 */
auth: AuthMethod, 
/**
 * The device and line settings of a serial session.
 */
serial?: SerialOptions, 
/**
 * Profiles to connect through, in order (OpenSSH's `ProxyJump a,b`). Each hop uses its
 * own profile's address and authentication, but not that profile's jump hosts. SSH
 * profiles only; SSH and Telnet sessions can use them.
 */
jumpHosts: Array<string>, 
/**
 * The id of the proxy to connect through; SSH and Telnet. Not kept with jump hosts,
 * where the first jump host's own proxy is used (see [`Route`]).
 */
proxy?: string, 
/**
 * Seconds between keepalive messages; 0 disables them. Three unanswered ones in a row
 * drop the connection. Telnet uses TCP keepalives.
 */
keepaliveInterval: number, 
/**
 * Reconnect automatically when an established connection is lost.
 */
autoReconnect: boolean, 
/**
 * Edited separately through [`ProfileStore::set_forwards`]; `save` keeps them.
 */
forwards: Array<ForwardRule>, 
/**
 * The folder the session is in; `None` for the top level. Changed with
 * [`ProfileStore::move_item`]; `save` keeps it.
 */
folder?: string, 
/**
 * Record a session log from the start of each connection.
 */
autoLog: boolean, 
/**
 * The quick command group its tabs show first; `None` for the default group.
 */
commandGroup?: string, 
/**
 * Let the remote shell use the local SSH agent (OpenSSH's `ForwardAgent`).
 */
forwardAgent: boolean, 
/**
 * The remote side's character encoding, one of [`crate::encoding::SUPPORTED`].
 */
encoding: string, 
/**
 * The terminal type the remote shell is told (`TERM`).
 */
termType: string, 
/**
 * Environment variables for the remote shell (OpenSSH's `SetEnv`); the server only
 * accepts those its `AcceptEnv` allows.
 */
env: Array<EnvVar>, 
/**
 * Commands typed into each new shell, in order, each once the shell shows a prompt.
 * Sent by the frontend.
 */
loginCommands: Array<string>, 
/**
 * Terminal appearance for this session; unset values follow the settings.
 */
appearance?: ProfileAppearance, };

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
 * SOCKS5 and HTTP: empty for a proxy without authentication. The password is in the
 * keychain.
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
 * What right-clicking the terminal does.
 */
export type RightClick = "menu" | "paste";

/**
 * A saved session or proxy, with the error storing its password gave, if any. It is saved
 * either way: the dialog then goes on editing it, so that saving again doesn't add another.
 */
export type Saved<T> = { saved: T, passwordError: CommandError | null, };

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
export type SessionCandidate = { id: string, name: string, protocol: Protocol, host: string, port: number, username: string, serial: SerialOptions, 
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
autoForwards: Array<ForwardRule>, };

export type SessionEvent = { "type": "connected" } | { "type": "closed", reason: CloseReason, error: CommandError | null, status: number | null, } | { "type": "forward", ruleId: string, state: ForwardState, } | { "type": "zmodem", phase: ZmodemPhase, } | { "type": "log", path: string | null, error: CommandError | null, };

export type SetAsideFile = { path: string, 
/**
 * Where the file was moved; `None` if moving it failed too.
 */
movedTo: string | null, error: string, };

export type Settings = { appearance: Appearance, textSize: TextSize, terminal: TerminalSettings, tabs: TabSettings, sidebar: SidebarSettings, files: FileSettings, zmodem: ZmodemSettings, logs: LogSettings, };

export type SidebarSettings = { 
/**
 * Show the most recently opened sessions above the list.
 */
showRecent: boolean, };

export type TabSettings = { 
/**
 * Show the title set by the shell (OSC 0 / 2) instead of the session name.
 */
followRemoteTitle: boolean, 
/**
 * Ask before closing tabs that are connected or running a program.
 */
confirmClose: boolean, };

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
 * Let screen readers read the terminal and announce new output (xterm.js's
 * `screenReaderMode`); off by default, since it slows down terminals with much output.
 */
screenReader: boolean, };

/**
 * The size of the interface's text (not the terminal's, which has its own font size).
 */
export type TextSize = "normal" | "large" | "larger";

export type TransferProgress = { transferred: number, total: number, filesDone: number, filesTotal: number, current: string, };

/**
 * A session or a folder, as dragged in the sidebar.
 */
export type TreeItem = { "kind": "profile", id: string, } | { "kind": "folder", id: string, };

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
