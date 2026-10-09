# Changelog

User-facing changes in each release, generated from the commit history with [git-cliff](https://git-cliff.org).

## [1.6.6] - 2026-10-09

### Bug Fixes

- Port forwarding: a session's rules run once, even with several tabs of the session open. Starting, stopping, editing or deleting a rule from any tab acts where it runs, every tab shows its state, and a tab that connects later no longer fails to bind ports another tab already forwards. Closing the last tab of a connection that runs rules, while another tab of the session is connected, asks whether to keep them running there. Reconnecting a tab by hand starts the rules it was running again, including those started by hand.
- Port forwarding: local and dynamic rules on `localhost` listen on both 127.0.0.1 and ::1, so clients connecting to 127.0.0.1 are no longer refused.
- Port forwarding: stopping or editing a remote rule while it is still starting no longer leaves the server listening, which made the rule fail with "address in use" until reconnecting.

## [1.6.5] - 2026-10-09

### Bug Fixes

- SFTP: downloads over slow links (below about 400 KB/s) no longer fail with "Timeout", and neither does listing a folder during a download.
- SFTP: saving a file opened in a local editor again while it is still uploading no longer reports that it changed on the server, and can no longer leave old data at the end of the file.
- SFTP: uploading a folder no longer follows links to folders, which could walk the whole disk (`loop -> ..`).
- ZMODEM: sending files to a receiver that acknowledges each block no longer stops after the first one.
- ZMODEM: receiving survives a pause of more than 10 seconds in the data, and gives up on data that arrives damaged every time instead of retrying for ever.
- ZMODEM: output that only looks like the start of a transfer (`cat` of a binary file) is shown after at most 2 seconds instead of being dropped, and Ctrl+C meanwhile reaches the program.
- ZMODEM: closing or reconnecting a tab while rz or sz waits for a file choice ends the transfer, which kept running (with the tab's log file) until the app quit.
- Telnet: sending files with rz to a server that doesn't echo no longer prints the transfer's data in the terminal and the session log.

## [1.6.4] - 2026-10-09

### Bug Fixes

- A proxy command no longer receives a host or user name the shell would run as a command (as from a session imported from someone else's file); such names are refused, as in OpenSSH.
- Importing from ssh_config: `Match` blocks are no longer applied to the host before them, `Include`d files are read, a `ProxyJump` written as `user@alias` or `alias:port` uses the alias's settings, and a jump host's own jump hosts are kept. Hosts are imported from the file that was loaded, even if the path was edited afterwards.
- Importing sessions from a file checks them as saving does (a remote forward without a bind address no longer listens on every interface of the server), lists forwarding rules that start on connecting, handles missing or repeated ids, and no longer takes a Telnet session of the same name for an SSH jump host.
- When a session or proxy is saved but the keychain fails, saving again updates it instead of adding a second copy.
- Windows: opening ZShell again brings the running window forward instead of starting a second instance, which could overwrite the first one's changes.
- Spaces typed in the terminal font name are kept.
- Enter and Escape used with an input method (to confirm or cancel a candidate) no longer submit, close a dialog or end a rename.

## [1.6.3] - 2026-10-09

### Bug Fixes

- After a connection drops while a program such as tmux or vim has the mouse or the alternate screen on, the reconnected shell no longer gets clicks typed in as text, and the message saying how the session ended stays visible.
- A password pasted at a prompt after reconnecting is no longer ignored.
- Connecting gives up after 30 seconds on a server, proxy command or proxy that stops answering, so automatic reconnection tries again instead of waiting for ever.
- Closing a duplicated or split SSH tab closes its shell on the server, which could otherwise run into the server's session limit.
- SSH: a wrong passphrase for a key in the older PEM format skips the key instead of ending automatic authentication.
- SSH: a line in `~/.ssh/known_hosts` that isn't UTF-8 no longer makes every connection fail.
- Sync input no longer sends the terminal's answers to programs' queries to the other panes.
- Tab shortcuts no longer switch or open tabs behind an open dialog.
- Reconnecting a serial session no longer fails with the device busy.

## [1.6.2] - 2026-10-09

### Bug Fixes

- Closing a local terminal tab no longer submits a line that was typed but not entered (to the shell, or to a program such as psql).
- Files downloaded, dragged out, opened for editing or received by ZMODEM always stay in the folder they are saved to, whatever name the server or sender gives them. On Windows, characters and names Windows doesn't allow are replaced.
- A ZMODEM transfer with an out-of-range file date no longer crashes the app.
- If a sessions, folders, proxies, settings or quick commands file can't be read, ZShell still starts: the file is kept under a name ending in `.bad-` and the date, and a notice lists it. Data files are also written to disk before they replace the old ones, so a power loss can't leave them empty.
- SSH: when the agent refuses to sign (a denied confirmation or an untouched security key), authentication moves on to the next key or method instead of hanging at "connecting".
- Plain-text session logs no longer use unbounded memory on very long lines or large cursor movements.
- Typed numbers in Settings, such as Scrollback lines or the font size, take effect on Enter or when leaving the field, rather than on every key (typing 20000 used to cut every terminal's history to 2 lines on the way).

## [1.6.1] - 2026-10-09

### Features

- Settings › About shows the folder where sessions and settings are saved, with a button that opens it in Finder or File Explorer.

### Bug Fixes

- Tooltips appear after 0.2 s, each with the same delay, instead of at once when moving from one to the next.

## [1.6.0] - 2026-10-09

### Features

- Settings: a navigation list beside the settings scrolls to each section, and a search box filters the settings by their names, descriptions and choices (⌘F / Ctrl+F). The dialog closes with × in the corner (changes apply immediately), and Restore Defaults asks for a second click.
- Settings › Appearance › Text size makes the app's own text larger (Large or Larger); the terminal keeps its own font size.
- Settings › Keyboard Shortcuts lists the app's shortcuts for your platform.
- Number fields such as the terminal font size have ▲ / ▼ buttons, and ↑ / ↓ step the value (ten steps with Shift).
- On Windows and in full screen on macOS, the app's icon and name fill the top-left corner of the window.
- Tooltips appear sooner, and at once when moving from one to the next.
- The sidebar's top row keeps only New Session and Import/Export; create folders from the context menu of the session list or of a folder.

## [1.5.1] - 2026-10-09

### Bug Fixes

- Host keys are checked against `known_hosts` as OpenSSH does: a server whose key is on any line for the host connects, even if another line has a different key of the same type (it used to be refused as a changed key).
- Keys marked `@revoked` in `known_hosts` are refused with a warning, and host patterns with wildcards (`*`, `?`) and negations (`!`) apply.

## [1.5.0] - 2026-10-09

### Features

- Known hosts: Settings › Known Hosts lists the host keys in `~/.ssh/known_hosts` with their key types and SHA256 fingerprints, and removes the ones you choose, keeping the previous file as `known_hosts.old` like `ssh-keygen -R`. Hashed host names are found by searching for the host name.
- When a server's host key has changed, the warning in the terminal names the right line of `known_hosts` and tells how to remove the old key, in the settings or with the `ssh-keygen` command shown. The connection is still refused, as in OpenSSH.

## [1.4.1] - 2026-10-09

### Bug Fixes

- Split Right and Split Down are disabled in the menus when a pane is too small to split, and the split shortcuts briefly outline such a pane instead of doing nothing.

## [1.4.0] - 2026-10-09

### Features

- Split panes: split a tab right (⌘D / Alt+Shift+=) or down (⇧⌘D / Alt+Shift+-) from the keyboard or the tab and terminal menus. Each pane is a session of its own; a split SSH pane opens another shell on the same connection, without connecting again. A saved session can also be opened in a new pane from its menu in the session list.
- Drag the dividers between panes to resize them, or double-click one to make the panes equal. ⌥⌘ / Ctrl+Alt with an arrow key moves to the pane on that side.
- ⌘W / Ctrl+Shift+W closes the focused pane of a split tab, and a local shell that exits closes its pane. The tab shows the focused pane's title and status, and the file and port forwarding panels show its connection.
- The compose bar can send to all panes of the current tab, and lists split tabs with their panes to choose from.

## [1.3.3] - 2026-10-09

### Bug Fixes

- Hover hints appear sooner, after 0.3 seconds.

## [1.3.2] - 2026-10-09

### Bug Fixes

- Hover hints now show on macOS: on tabs, sessions, toolbar buttons and everywhere else that has one. The system web view on macOS never showed them.
- The question mark icons in session settings show their explanation right away, also when focused with the keyboard.

## [1.3.1] - 2026-10-09

### Bug Fixes

- The private key field has a Choose… button. A key in the home folder is saved as `~/…`, so exported sessions keep working on other computers.
- Where `~` appears in a session's settings, a question mark icon shows the folder it stands for (on Windows, `C:\Users\<name>`), and for the key path the full path.
- The warning about a changed host key names the actual `known_hosts` file instead of `~/.ssh/known_hosts`.

## [1.3.0] - 2026-10-09

### Features

- Proxies: connect SSH and Telnet sessions through a SOCKS5 or HTTP proxy, with an optional user name and password (kept in the system keychain, or asked in the terminal), or through a proxy command like OpenSSH's `ProxyCommand`, whose error output appears in the terminal. Proxies are saved once in Settings and chosen on a session's Connection page.
- Only the first connection goes through the proxy: with jump hosts, the first jump host's own proxy is used, as in OpenSSH.
- Importing from `~/.ssh/config` brings `ProxyCommand` along as a proxy, and turns `ProxyCommand ssh -W %h:%p host` into a jump host. Exported sessions include their proxies.

## [1.2.0] - 2026-10-08

### Features

- Telnet sessions, for network devices and older systems: the terminal type and window size are reported to the server, a saved user name and password are typed at the login prompts, and they can go through SSH jump hosts.
- Serial sessions: pick a port (`/dev/cu.*` on macOS, COM ports on Windows) and its baud rate, data bits, parity, stop bits and flow control. When a USB adapter is unplugged, the session reconnects once it is back. Virtual ports such as QEMU's `-serial pty` work too.
- Telnet and serial tabs can send a break from the tab's menu. ZMODEM, session logs, character encodings and login commands work in them as in SSH sessions.
- Quick connect accepts `telnet host[:port]` and `telnet://host[:port]`, and the session list shows each session's protocol in its address.

## [1.1.0] - 2026-10-08

### Features

- Settings has an About section with the version, the privacy policy and the licenses of the third-party software ZShell includes, searchable by package.
- Windows: in S mode, which doesn't allow command-line shells, the ways to open a local terminal are hidden.

## [1.0.0] - 2026-10-08

The first release, for macOS (Apple Silicon and Intel) and Windows 10 1809 or later.

### SSH

- Password, private key, SSH agent and keyboard-interactive authentication, plus an automatic mode that tries them in OpenSSH's order; host keys checked against `~/.ssh/known_hosts`; saved passwords kept in the system keychain.
- One connection per tab, shared by the terminal, the file panel and port forwarding; duplicating a tab opens another shell on the same connection.
- Jump hosts (like `ProxyJump`), keepalives and automatic reconnection.
- Local (`-L`), remote (`-R`) and dynamic SOCKS (`-D`) port forwarding, saved with the session and optionally started on connect, with live status.
- Per-session agent forwarding, character encodings (GBK, GB18030, Big5, Shift_JIS and others) for the terminal, SFTP and ZMODEM file names, terminal type and environment variables.

### Files

- SFTP panel: sorting, filtering and hidden files; multiple selection; upload and download with progress, to the download folder or a place you choose; drag files in, out to Finder or Explorer, or onto a folder to move them; edit a remote file in your local editor and upload it on each save; rename, delete, new folder and permissions.
- ZMODEM (`rz` / `sz`) in SSH sessions and local terminals.

### Terminal and tabs

- Local terminals: your login shell on macOS, PowerShell on Windows.
- Clipboard shortcuts, context menus, copy on select, right-click paste, confirmation before multi-line pastes, Option as Meta on macOS, and search with regular expressions.
- Built-in color schemes, fonts, and light and dark appearance; a session can have its own colors (a red background for production, marked on its tabs too) and font.
- Tabs in the window's title bar: drag to reorder, rename, duplicate, follow the shell's title, and confirm before closing busy tabs.

### Sessions and productivity

- Nested folders arranged by drag and drop, search, quick connect with `user@host[:port]`, recent sessions, duplicate, and export/import without passwords.
- Import hosts from `~/.ssh/config`.
- Commands typed after login, waiting for the shell's prompt before each one.
- Compose bar to send a command to several tabs, or sync typing to them.
- Quick commands in groups, as buttons below the terminal, in its menu and in a searchable palette.
- Session logs, recorded automatically or from a tab's menu, as plain text or raw, with cleanup after a number of days.
