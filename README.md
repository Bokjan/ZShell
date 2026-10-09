<img src="src-tauri/icons/128x128.png" width="96" alt="">

# ZShell

A cross-platform (macOS / Windows) SSH, Telnet and serial terminal inspired by Xshell, built with Tauri 2, Rust and xterm.js.

## Features

- **SSH sessions**: password, private key, SSH agent and keyboard-interactive (two-factor) authentication, plus an automatic mode that tries them in OpenSSH's order. Host keys are checked against `~/.ssh/known_hosts`, and saved passwords are kept in the system keychain.
- **One connection per tab**: the terminal, the file panel and port forwarding share a single SSH connection, so you authenticate once. Duplicating a tab opens another shell on the same connection.
- **Jump hosts**: connect through one or more other sessions, like OpenSSH's `ProxyJump`, for SSH and Telnet alike.
- **Proxies**: SOCKS5 and HTTP proxies (with an optional user name and password) and proxy commands like OpenSSH's `ProxyCommand`, saved once and chosen per session.
- **Telnet**: for network devices and older systems, with the terminal type and window size reported to the server, an optional saved user name and password typed at the login prompts, and a break from the tab's menu.
- **Serial consoles**: open a serial port (`/dev/cu.*` on macOS, `COM` ports on Windows) with the baud rate, data bits, parity, stop bits and flow control you set; the session reconnects by itself when a USB adapter is unplugged and plugged back in. Virtual ports such as QEMU's `-serial pty` work too.
- **Stays connected**: keepalives detect dead connections, and lost connections are re-established automatically.
- **SFTP file panel**: browse with sorting, a name filter and hidden files on or off; select several items with ⇧ / ⌘ (Ctrl) and act on them from the context menu or the keyboard; upload and download with progress, to the download folder or a place you choose; drag files in to upload (onto a folder, too), out to Finder or Explorer to download, or onto a folder to move them; open a remote file in your editor and have it uploaded each time you save; rename, delete, create folders and change permissions.
- **ZMODEM**: `rz` and `sz` in the terminal, for hosts without SFTP such as those behind bastion hosts, over SSH, Telnet and serial lines and in local terminals. Choose files or drop them on the terminal for `rz`; files from `sz` go to the download folder (or ask where).
- **Port forwarding**: local (`-L`), remote (`-R`) and dynamic SOCKS (`-D`) rules, saved with the session, optionally started on connect, with live status.
- **Import from `~/.ssh/config`**: hosts, users, ports, keys, jump hosts, proxy commands, forwards, agent forwarding and `SetEnv`.
- **Session management**: nested folders arranged by drag and drop, search, quick connect by typing `user@host[:port]` or `telnet host[:port]`, recently opened sessions, duplicate, and export/import to a file (without passwords) for backups and other computers.
- **Local terminals**: your login shell on macOS, PowerShell on Windows.
- **Terminal**: context menu, copy on select, right-click to paste, confirmation before pasting multiple lines, Option as Meta on macOS, search with regular expressions, built-in color schemes, custom fonts, light and dark appearance.
- **Per-session settings**: SSH agent forwarding; character encodings such as GBK, GB18030, Big5 and Shift_JIS for older servers and network devices (over SSH, Telnet or a serial line), converted in the terminal and in SFTP and ZMODEM file names; the session's own color scheme, background color (say, red for production, also marked on its tabs) and font; commands typed after login once the shell shows its prompt (`sudo -i`, `cd /srv/app`); the terminal type and environment variables sent to the server.
- **Session logs**: record what a terminal shows to a file, automatically for chosen sessions (and optionally local terminals) or from a tab's menu, as plain text or raw, with old logs cleaned up after a number of days if you like.
- **Quick commands**: buttons below the terminal for commands you type often, in groups of your own, also in the terminal's menu and a searchable palette. A session can choose the group its tabs show first.
- **Send to several tabs**: a compose bar sends a command to the current tab, all tabs or selected ones, and can sync what you type in the terminal to them.
- **Tabs**: drag to reorder, rename, duplicate, follow the title set by the shell, and confirmation before closing connected tabs or tabs running a program. The tab bar sits in the window's title bar on both macOS and Windows.

Planned next: connecting through proxies and `ProxyCommand`, split panes, and managing known hosts. See the [roadmap](docs/ROADMAP.md) (in Chinese) and the [changelog](CHANGELOG.md).

## Installation

Download the latest version from the [releases page](https://github.com/Bokjan/ZShell/releases):

| Platform | File |
|---|---|
| macOS (Apple Silicon and Intel) | `ZShell_<version>_universal.dmg` |
| Windows installer | `ZShell_<version>_x64-setup.exe` or `ZShell_<version>_x64_en-US.msi` |
| Windows, no installation | `ZShell_<version>_x64_standalone.exe` |

Local terminals on Windows need Windows 10 version 1809 or later. The standalone executable needs the WebView2 runtime, which Windows 11 and up-to-date Windows 10 already include; it still keeps its settings in `%APPDATA%`.

The builds are not signed with a developer certificate yet, so the system warns on first launch:

- **macOS**: after moving ZShell to Applications, open it once; when macOS refuses, go to System Settings → Privacy & Security and click "Open Anyway". Alternatively, remove the quarantine flag in Terminal:

  ```bash
  xattr -dr com.apple.quarantine /Applications/ZShell.app
  ```

- **Windows**: when SmartScreen shows "Windows protected your PC", click "More info", then "Run anyway".

## Usage

Add a session with **+** in the sidebar (choose SSH, Telnet or Serial as its protocol), or import hosts from your SSH config, then double-click a session (or select it and press Enter) to open it in a new tab. **+** at the end of the tab bar opens a local terminal. To connect without saving a session, type `user@host` or `user@host:port` (or `telnet host:port`) in the search box and press Enter. Host key confirmations, passwords and passphrases are asked in the terminal, as `ssh` does. After a disconnection, press Enter in the terminal to reconnect. Right-click the terminal or a tab for more actions; double-click a tab to rename it.

| Action | macOS | Windows |
|---|---|---|
| Search sessions | ⌘K | Ctrl+Shift+K |
| New local terminal | ⌘T | Ctrl+Shift+T |
| Copy | ⌘C | Ctrl+Shift+C, or Ctrl+C with a selection |
| Paste | ⌘V | Ctrl+Shift+V or Ctrl+V |
| Next / previous tab | Ctrl+Tab / Ctrl+Shift+Tab | Ctrl+Tab / Ctrl+Shift+Tab |
| Go to tab 1–8 / last tab | ⌘1–8 / ⌘9 | Alt+1–8 / Alt+9 |
| Close tab | ⌘W | Ctrl+Shift+W |
| Settings | ⌘, | Ctrl+, |
| Find in terminal | ⌘F | Ctrl+Shift+F |
| Show/hide the compose bar | ⇧⌘I | Ctrl+Shift+I |
| Quick command palette | ⇧⌘J | Ctrl+Shift+J |
| Show/hide the file panel | ⇧⌘E | Ctrl+Shift+E |
| Show/hide the port forwarding panel | ⇧⌘P | Ctrl+Shift+P |

### Where data is stored

- Sessions and settings: `~/Library/Application Support/org.boyin.zshell/` on macOS, `%APPDATA%\org.boyin.zshell\` on Windows (`profiles.json`, `folders.json`, `settings.json`, `commands.json` and `logs.json`).
- Saved passwords: the macOS Keychain or the Windows Credential Manager, never in those files. Passphrases for private keys are asked each time and never saved.
- Known hosts: `~/.ssh/known_hosts`, shared with OpenSSH.

## Development

Requirements: Rust (stable), Node.js and pnpm.

```bash
pnpm install
pnpm tauri dev      # run in development mode
pnpm tauri build    # build release bundles
```

Documentation for contributors is in Chinese:

- [docs/ROADMAP.md](docs/ROADMAP.md): product roadmap and milestones
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): architecture and design decisions
- [docs/I18N.md](docs/I18N.md): language policy and internationalization
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): testing, conventions, CI and releases

Recommended editor setup: [VS Code](https://code.visualstudio.com/) with the [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) and [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer) extensions.

## License

Copyright © 2026 Boyin Chen. All rights reserved.

ZShell is free to use, but it is not open source. The source code is published for transparency and reference only; no license is granted to copy, modify or redistribute it. The released builds (from the [releases page](https://github.com/Bokjan/ZShell/releases) and the Microsoft Store) may be used free of charge, including for work.

See [PRIVACY.md](PRIVACY.md) for the privacy policy.
