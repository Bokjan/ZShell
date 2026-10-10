# <img src="src-tauri/icons/128x128.png" width="36" alt=""> ZShell

[![CI](https://github.com/Bokjan/ZShell/actions/workflows/ci.yml/badge.svg)](https://github.com/Bokjan/ZShell/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/Bokjan/ZShell)](https://github.com/Bokjan/ZShell/releases/latest)
![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue)

A cross-platform (macOS / Windows) SSH, Telnet and serial terminal, built with Tauri 2, Rust and xterm.js.

## Features

- **SSH**: password, private key, agent and keyboard-interactive authentication (or all of them in OpenSSH's order), host keys checked against `~/.ssh/known_hosts`, jump hosts, SOCKS5 and HTTP proxies or proxy commands, keepalives and automatic reconnection. Hosts can be imported from `~/.ssh/config`.
- **One connection per tab**: the terminal, the file panel and port forwarding share a single SSH connection, and duplicated tabs and split panes reuse it, so you authenticate once.
- **Telnet and serial consoles** for network devices and older systems, with saved logins typed at the prompts, and serial sessions that reconnect when a USB adapter is plugged back in.
- **Files**: an SFTP panel with drag and drop in and out, transfers with progress, and remote files edited in your own editor and uploaded on save; `rz` and `sz` (ZMODEM) for hosts without SFTP.
- **Port forwarding**: local, remote and dynamic SOCKS rules (`-L`, `-R`, `-D`) saved with the session, with live status.
- **Sessions**: nested folders, search and quick connect, export and import, and per-session character encodings (GBK, Big5, Shift_JIS…), colors, fonts, commands run after login and environment variables.
- **Terminal and tabs**: local terminals, split panes, tabs in the title bar, a full screen showing only the terminals, search, color schemes, copy on select, right-click paste and session logs.
- **Many hosts at once**: quick command buttons and a command palette, and a compose bar that sends to several terminals or syncs what you type.

Planned next: encrypted storage for sessions and passwords, and locking the app. See the [roadmap](docs/ROADMAP.md) (in Chinese) and the [changelog](CHANGELOG.md).

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

## Getting started

Add a session with **+** at the top of the sidebar, or import your SSH config from the menu next to it, then double-click the session to connect. To connect without saving a session, type `user@host[:port]` or `telnet host[:port]` in the search box and press Enter. Host keys, passwords and passphrases are asked in the terminal, as `ssh` does; after a disconnection, press Enter to reconnect. Right-click terminals, tabs and sessions for more, and see Settings → Keyboard Shortcuts for the keys.

Sessions and settings are saved in `~/Library/Application Support/org.boyin.zshell/` on macOS and `%APPDATA%\org.boyin.zshell\` on Windows. Saved passwords go to the macOS Keychain or the Windows Credential Manager, and host keys to `~/.ssh/known_hosts`, shared with OpenSSH. See [PRIVACY.md](PRIVACY.md) for the details.

## Development

Requirements: Rust (stable), Node.js and pnpm.

```bash
pnpm install
pnpm tauri dev            # run in development mode
pnpm licenses:generate    # third-party license notices shown in Settings > About (needs cargo-about)
pnpm tauri build          # build release bundles
```

Documentation for contributors is in Chinese:

- [docs/ROADMAP.md](docs/ROADMAP.md): product roadmap and milestones
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): architecture and design decisions
- [docs/I18N.md](docs/I18N.md): language policy and internationalization
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): testing, conventions, CI and releases

## Feedback

Bug reports and feature requests are welcome as [issues](https://github.com/Bokjan/ZShell/issues). Pull requests are generally not accepted; please discuss a change in an issue first. See [CONTRIBUTING.md](CONTRIBUTING.md), and [THANKS.md](THANKS.md) for the people who have helped.

## License

Copyright © 2026 Boyin Chen. All rights reserved.

ZShell is free to use and its source code is public, but it is source-available rather than open source. The released builds may be used free of charge for any purpose, including at work, and you may study, build and modify the source code for your own use, but not distribute it or any build of it. See [LICENSE.md](LICENSE.md) for the terms.
