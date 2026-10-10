# Changelog

Changes in each release, generated from the commit history with [git-cliff](https://git-cliff.org): what changed for users, then internal restructuring.

## [2.4.0] - 2026-10-10

### Features

- The session dialog asks before throwing away changes when it is closed with Escape or a click outside (Cancel still closes at once), and saving with a field that can't be saved goes to its page and puts the cursor in it.
- A tab whose file panel has a question waiting (a file edited in another app also changed on the server) is marked, and so is the Files button; screen readers hear it with the tab's name.

### Bug Fixes

- Renaming, creating a folder, changing permissions or deleting in the file panel no longer goes back to the folder it started in when you have opened another one meanwhile (deleting a large folder).
- A session log that stops because it can't be written (a full disk, a network drive gone) now says so in the terminal, and the tab no longer shows it as recording. A log being continued after a reconnection is never taken for an old one and deleted.
- Closing a Telnet tab while a paste waits for a device that stopped reading now closes the connection (and a proxy command's process) instead of leaving them behind.
- Host names with `, * ? ! [ ]` are refused, also in quick connections, and accepted host keys are written by ZShell itself: connecting to `*` through a command proxy that ignores the host could otherwise trust a key for every host. An empty known_hosts no longer starts with a blank line.
- Ports in forwarding rules are whole decimal numbers: `0x16` is no longer taken as port 22.
- The quick command bar counts the terminals its commands go to ("Sends to 3 terminals") rather than calling them tabs.
- Dragging files out of the file panel: an item asked for just as the drop was taken as over no longer downloads after the transfer was reported done.
- The page runs under a strict content security policy, as a safeguard: only the app's own scripts run.

### Performance

- Listing a folder with many symlinks (`/usr/lib`) looks them up in parallel rather than one after another.
- A dynamic (-D) forwarding rule used by a browser no longer sends hundreds of updates a second to every tab of the session.
- A shell updating its tab's title (at every prompt) no longer redraws every other tab and terminal.

### Internal

- Storage: the JSON files are read and written in one place (persist.rs); a file that can't be written is reported as such, settings that didn't change aren't rewritten, and hand-written session files may leave out settings that have defaults or repeat ids. Session lists no longer wait for the files of a change to be written.
- The backend's commands are split by area, and connections to jump hosts take their options as one value.
- The frontend's large components are split into parts with their logic in tested modules: the app shell's hooks, the file panel (listing, transfers, drag and drop, selection), the session dialog's form, the settings' sections, and shared fields (password, folder, port, import list).
- The compose bar, quick commands and synced typing send through each pane's session, which decides in one place whether its shell is up.
- More tests: pane layouts, the compose scope, quick connection addresses, reconnecting with forwarding rules, output acknowledgement, where a session's forwarding rules run, and asking again for a rejected proxy password.

## [2.3.6] - 2026-10-10

### Bug Fixes

- A host recorded in known_hosts only with a key of another type (ECDSA in many older files) is no longer asked about as if it were new: ZShell asks the server for the type it knows, as OpenSSH does. When a server shows a type not recorded for a host that has others, the question says so and lists the recorded keys, so a server pretending to be a known one doesn't get the plain first-connection question.
- If folders.json or proxies.json can't be read at startup, sessions keep their folders and proxies, which come back once the file is restored: they no longer lose them for good, and a session whose proxy is missing doesn't connect without it. A settings or session file that can't be read at all (open in another program) is never written over; changes that would write it ask you to restart ZShell.
- Importing sessions shows, on each session, the jump hosts imported with it and the commands their proxies run, and the jump hosts are shown checked. A sessions file that changes after the import dialog opens is no longer imported.
- Importing an SSH config that includes itself shows an error rather than closing ZShell, and `Include config.d/*` skips hidden files such as .DS_Store.
- A ZMODEM upload (`rz`) of several files stops when one can't be read midway, rather than appending the next file to it on the server and reporting it sent.

## [2.3.5] - 2026-10-10

### Bug Fixes

- Screen readers name every icon button (they read the glyph, or nothing while the pointer was on the button), including which rule or jump host a button acts on, and say whether toggle buttons such as the compose bar, quick commands and the Files and Forwards panels are on. The port forwarding on/off switch had no name at all.
- Icons are drawn the same on macOS and Windows: the arrows, close buttons, edit and delete buttons, check marks and the file list's folder and file icons were characters and emoji that looked different in each platform's fonts. Icons are also centered on the text beside them at every text size.
- Errors in dialogs and at the top of the session list, file panel and forwarding panel are read out as they appear, and a panel's error can be dismissed from the keyboard.
- Screen readers say what happens away from the focus: what the compose bar and quick commands sent, the number of matches when finding in the terminal, why a pane can't be split, and that saving a file edited in another app failed or found the file changed on the server.
- More works from the keyboard: the file panel's and quick commands' menus open with the menu key or Shift+F10; the settings' choices (appearance, text size, color scheme, right-click) and the session dialog's pages move with the arrow keys; the dividers between panes and the edges of the side panel and session list can be focused and moved with the arrow keys; and the arrow keys in the quick command palette and the license list are read out.
- On Windows, opening the file panel's menu with the menu key no longer clears the selection and shows the folder's menu.
- Pressing ↓ then Enter in the session search with no matches no longer throws an error.
- An invalid value for Change Permissions is shown in its dialog, rather than behind it where it seemed nothing happened.
- Pressing Escape to cancel an input method's candidate no longer abandons renaming a file.
- The SFTP filter field shows when it has the keyboard focus, and in a narrow pane the find bar takes two rows rather than cutting off its buttons.

### Internal

- The native window's background color comes from the theme's colors.
- OpenSSH's prompts, kept in English on purpose, are in one place with the reason.
- Every icon is drawn in one place, and a test fails on an icon drawn elsewhere or made of a character.

## [2.3.4] - 2026-10-10

### Bug Fixes

- With Sync input on in the compose bar, what is typed in a terminal that is still connecting (a password, a key passphrase, the answer to a host key) is no longer sent to the other terminals, where it was run in their shells. Typing is synced once the terminal is connected.
- Resizing the window while an SSH terminal was busy printing a lot of output could freeze that connection for good, together with its other tabs, file panel and port forwarding. The output now keeps flowing and the new size reaches the server.

## [2.3.3] - 2026-10-09

### Bug Fixes

- Links that open in the browser (the privacy policy, and package homepages in the third-party licenses) are marked with an icon, and screen readers say so.
- Settings › About links to ZShell's new license, which spells out the terms: the released builds are free for any use, including at work, and the source code may be read, built and modified for your own use but not redistributed.

## [2.3.2] - 2026-10-09

### Bug Fixes

- The session log folder (`ZShellLogs` in Documents by default) is no longer created just by opening the settings; it is created when the first log is written.

## [2.3.1] - 2026-10-09

### Bug Fixes

- On macOS, the refresh buttons in the file panel and next to the serial device are no longer drawn tiny.

## [2.3.0] - 2026-10-09

### Bug Fixes

- A program printing a lot of output over SSH or Telnet (`cat` of a large file) no longer makes ZShell's memory grow until the terminal drops output: the session waits for the terminal to catch up, as local terminals already did.
- A ZMODEM download to a disk slower than the connection no longer fills memory.
- The message saying how a session ended always comes after its last output.
- A slow log folder (a network drive) no longer slows down the session being logged; if the log falls too far behind, it notes how much output is missing.
- Listing a folder in the file panel no longer waits for a large upload or download to finish.

## [2.2.0] - 2026-10-09

### Bug Fixes

- When the window's page reloads or its window closes, its sessions end instead of running on unseen in the background.

## [2.1.0] - 2026-10-09

### Features

- Servers that require more than one authentication method, such as a key and then a verification code (`AuthenticationMethods publickey,keyboard-interactive` for Google Authenticator or Duo) or a key and then a password, can be connected to: after each step, ZShell goes on with the methods the server still requires, as `ssh` does. Verification codes are asked in the terminal; the saved password only answers password prompts.
- Keyboard-interactive is tried before password when a server offers both, as `ssh` does.
- When a server requires a method ZShell cannot use, the error says which methods it requires.

## [2.0.0] - 2026-10-09

### Breaking Changes

- Sessions are saved in a new format, and ZShell 2.0 does not read the sessions of earlier versions: at the first start, the old `profiles.json` is renamed to `profiles.json.bad-<time>` and the session list starts empty. Folders, proxies, quick commands, settings and logs are kept; passwords saved for the old sessions stay in the keychain unused. Files exported by earlier versions cannot be imported either: recreate the sessions, or import them from `~/.ssh/config`.
- A session whose protocol is changed keeps only the settings of the new protocol: its port forwarding rules are removed when it stops being an SSH session, and running ones stop.

## [1.7.0] - 2026-10-09

### Features

- Settings › Terminal › Screen reader support lets VoiceOver, Narrator and NVDA read the terminal and announce new output. It is off by default, since terminals with a lot of output become slower.
- Dialogs, menus, tabs, the session list and the file panel work with screen readers and from the keyboard: dialogs keep the focus while open and give it back when they close; arrows move among tabs, Enter shows one, F2 renames it, and Shift+F10 or the menu key opens the menu of a tab, a session or a file; column titles in the file panel sort from the keyboard.
- Screen readers hear when a tab in the background connects or disconnects, when a file transfer finishes or fails, and when an address or path is copied.
- The keyboard focus is always visible. With Reduce Motion, transitions and flashes are off; when the system asks for more contrast, terminal colors too close to the background are adjusted; Windows contrast themes show the active tab and selected rows in the system highlight colors.
- Secondary and error text have more contrast, and primary buttons a darker blue.

### Bug Fixes

- With many tabs open, the first ones no longer fall back for good to the slower renderer: hidden tabs give up their WebGL renderer and take it back when shown.
- A dialog from a tab in the background (an edit conflict found when a file is saved) waits until that tab is shown, and no longer blocks shortcuts meanwhile.

## [1.6.12] - 2026-10-09

### Bug Fixes

- ZMODEM: files sent with `sz` are no longer saved without asking, since showing a file that happens to look like `sz` output (with `cat`) also starts a transfer. A bar over the terminal offers to save them in the download folder or another folder, or to cancel. The settings can save into the download folder without asking, or open the folder picker right away; "Ask where to save files received with sz" becomes the latter.
- ZMODEM: the bar over the terminal no longer cuts its message short.
- File panel: cancelling an upload over an existing file, or an upload or download failing part way, no longer destroys the file being replaced. Uploads, uploads of edited files and Download to… now write such a file under a temporary name beside it and replace it only once complete, keeping its permissions. Symbolic links, files owned by someone else and folders that don't allow new files are still written in place.

## [1.6.11] - 2026-10-09

### Bug Fixes

- Compose bar: a paste while syncing input reaches each pane the way its own terminal needs: panes whose program doesn't take bracketed pastes no longer receive the markers as text, and a multi-line paste asks first unless every pane inserts lines without running them.
- File panel: a conflict found while saving an edited file no longer replaces a delete or replace question already on screen; questions are asked one after another.
- Sessions: clicking Delete again in the session or proxy dialog while it is deleting no longer reports that the session was not found; saving and deleting go one at a time.
- Local terminal: a program that isn't reading its input (with a large paste waiting) no longer keeps the terminal from following window resizes.

### Performance

- Session logs are written to disk at most once a second rather than for every piece of output, and what is left is written within a second once the output pauses, so `tail -f` still follows them.

## [1.6.10] - 2026-10-09

### Bug Fixes

- Saving sessions and proxies, deleting or duplicating them, importing and exporting, and starting a session log no longer freeze the window while the keychain or the disk is slow, and a keychain prompt (as macOS shows for every saved password after an update) no longer holds up the other tabs' output, forwards and file transfers.
- Sessions: a change that fails to save (a full disk) leaves all session files as they were, instead of saving the folders but not the sessions in them.
- SSH: host keys are also checked against `~/.ssh/known_hosts2` and the system's `ssh_known_hosts` (in `/etc/ssh`, or `%ProgramData%\ssh` on Windows), as OpenSSH does: keys an administrator installed no longer have to be confirmed, and `@revoked` lines there apply.
- SSH: a connection lost while authenticating with an encrypted key or the SSH agent no longer stops automatic reconnection as if the login had been refused.
- File panel: it works again without reconnecting the tab after the server's SFTP process ends while the connection stays up.
- ZMODEM: receiving over a link that adds stray control characters (a modem, a terminal's answerback) no longer retries forever; they are dropped as noise, as in lrzsz.
- ZMODEM: on Windows, a character cut off by the start of a transfer is shown before the transfer's messages rather than after them.
- Session logs: continuing a log after reconnecting only ever appends to a log ZShell started.

## [1.6.9] - 2026-10-09

### Bug Fixes

- Terminal: pasting several lines into a disconnected tab no longer starts reconnecting it (and loses the text); only Enter reconnects.
- Tabs: panes closing at the same moment (all exiting after the compose bar sent them `exit`) can no longer bring each other back.
- Sessions: the Delete button of the session dialog, and of the proxy dialog, no longer stays waiting for confirmation, where a single click much later deleted; any click elsewhere cancels it. So does Delete All Logs in the settings.
- Sessions: number fields take digits only: a port typed as `0x16` was saved as 22, and `1e3` as 1000. A baud rate too large for the device settings is refused with a clear message.
- Sessions: saving a session before the quick command groups had loaded no longer drops its group.
- Sidebar: dropping a session on the Recent section no longer takes it out of its folder.
- Sidebar: dropping a folder at the bottom edge of an expanded folder puts it where the line shows, inside it, instead of after the whole folder.
- Settings: a log file name typed just before closing the settings with Esc or a click outside is kept.

## [1.6.8] - 2026-10-09

### Bug Fixes

- Proxies: a rejected proxy password can be typed three times, as for SSH, not four.
- Proxies: automatic reconnection stops when the proxy asks for credentials, rejects them or cannot use them, instead of retrying every 30 seconds for ever.
- SSH: closing a tab just as it finishes connecting no longer leaves the connection, and its automatically started forwards, open until the app quits.
- SSH: duplicating or splitting a tab whose connection died without being noticed yet (after the computer slept, say) connects anew instead of showing an error.
- Port forwarding: starting a rule from one tab while its connection is closing in another no longer leaves its port bound.
- Port forwarding: dynamic (SOCKS) forwards close connections that send no request within 30 seconds, such as port scans.
- Serial: on Windows, a proxy command or editor started while a serial tab is open no longer keeps the port busy after the tab closes.
- Importing from ssh_config: keys under the home folder are stored as `~/...` paths, so the sessions keep working when exported to another computer or user name.

## [1.6.7] - 2026-10-09

### Bug Fixes

- SFTP: downloading the same file twice in quick succession no longer gives both downloads the same name, where cancelling one deleted the other's file; on file systems that ignore case, downloading `README` and `readme` together no longer merges them. Received ZMODEM files and new log files are named the same way.
- SFTP: dragging several files out of the window shows the right file count instead of "3 / 1".
- SFTP: an upload that the server only reports as failed when the file is closed (NFS, disk quotas) is shown as failed instead of done.
- SFTP: opening a folder while a slow listing of another is still loading no longer shows the slow one when it arrives.
- SFTP: closing a pane cancels the file transfers running in it, which went on out of sight in split and duplicated tabs, and the close confirmation says so.
- SFTP: on macOS, a drag to another application that does not ask for every file no longer leaves its transfer running for good.
- SFTP: on Windows, dragging files onto a window that refuses them counts as cancelled instead of leaving the transfer running.
- ZMODEM: a file the sender abandons partway for another is deleted and reported instead of being left behind.
- ZMODEM: on serial lines with software flow control, an XON or XOFF inside a header no longer stalls the transfer for 10 seconds.
- ZMODEM: the start of a transfer no longer shows as stray characters when it arrives in pieces, as it often does on serial lines.
- ZMODEM: with `sz a; sz b`, the second transfer starts instead of showing as garbage.
- Sessions in another character encoding no longer lose a character cut off at the end of the session's output, and one cut off by the start of a ZMODEM transfer is no longer joined to what follows the transfer.

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
