# <img src="src-tauri/icons/128x128.png" width="36" alt=""> ZShell

[![CI](https://github.com/Bokjan/ZShell/actions/workflows/ci.yml/badge.svg)](https://github.com/Bokjan/ZShell/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/Bokjan/ZShell)](https://github.com/Bokjan/ZShell/releases/latest)
[![Website](https://img.shields.io/badge/website-zshell.boyin.org-C8553D)](https://zshell.boyin.org/)
[![Microsoft Store](https://img.shields.io/badge/Microsoft%20Store-ZShell-0078D4)](https://apps.microsoft.com/detail/9n502tzg8hk8)
![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-blue)

A cross-platform (macOS / Windows / Linux) SSH, Telnet and serial terminal, built with Tauri 2, Rust and xterm.js.

See the [ZShell website](https://zshell.boyin.org/) for an overview, or go straight to the [latest release](https://github.com/Bokjan/ZShell/releases/latest).

## Features

- **SSH**: password, private key, agent and keyboard-interactive authentication (or all of them in OpenSSH's order), host keys checked against `~/.ssh/known_hosts`, jump hosts, SOCKS5 and HTTP proxies or proxy commands, keepalives and automatic reconnection. Hosts can be imported from `~/.ssh/config`.
- **One connection per tab**: the terminal, the file panel and port forwarding share a single SSH connection, and duplicated tabs and split panes reuse it, so you authenticate once.
- **Telnet and serial consoles** for network devices and older systems, with saved logins typed at the prompts, and serial sessions that reconnect when a USB adapter is plugged back in.
- **Files**: an SFTP panel that opens in the terminal's folder and can follow it (on Linux servers, or shells that report it), with drag and drop in and out, transfers with progress, and remote files edited in your own editor and uploaded on save; `rz` and `sz` (ZMODEM) for hosts without SFTP.
- **Port forwarding**: local, remote and dynamic SOCKS rules (`-L`, `-R`, `-D`) saved with the session, with live status.
- **Sessions**: nested folders, search and quick connect, export and import, and per-session character encodings (GBK, Big5, Shift_JIS…), colors, fonts, commands run after login, environment variables and notes.
- **Terminal and tabs**: local terminals, split panes, tabs in the title bar, several windows that tabs move between with their sessions, a full screen showing only the terminals, search, color schemes, copy on select, right-click paste and session logs.
- **Many hosts at once**: quick command buttons and a command palette, and a compose bar that sends to several terminals or syncs what you type.
- **Security**: sessions and saved passwords encrypted with a master password or a key in the system's credential store, exports protected with a passphrase, and an app lock that covers the window with Touch ID, Windows Hello or the account password after a period of inactivity, when the computer locks, or on demand.
- **Languages**: English, Simplified Chinese, Japanese, German and French, following the system or chosen in Settings → Appearance. The Japanese, German and French translations have not been reviewed by native speakers yet: corrections are welcome as [issues](https://github.com/Bokjan/ZShell/issues).

Planned next: customizable shortcuts. See the [roadmap](docs/ROADMAP.md) (in Chinese) and the [changelog](CHANGELOG.md).

## Installation

On Windows (x64 and ARM64), the easiest way is the [Microsoft Store](https://apps.microsoft.com/detail/9n502tzg8hk8), which installs and updates ZShell for you without the warning below.

Otherwise, download the latest version from the [releases page](https://github.com/Bokjan/ZShell/releases):

| Platform | File |
|---|---|
| macOS (Apple Silicon and Intel) | `ZShell_<version>_universal.dmg` |
| Windows installer | `ZShell_<version>_x64-setup.exe` or `ZShell_<version>_x64_en-US.msi` |
| Windows, no installation | `ZShell_<version>_x64_standalone.exe` |
| Linux (Debian, Ubuntu) | `ZShell_<version>_amd64.deb` |
| Linux (Fedora) | `ZShell-<version>-1.x86_64.rpm` |

Local terminals on Windows need Windows 10 version 1809 or later. On Linux, ZShell is made for GNOME on Wayland, the default desktop of Debian, Ubuntu and Fedora; other desktops work, but may lack the idle lock. To open serial ports, add yourself to the group that owns them (`dialout` on most distributions). The standalone executable needs the WebView2 runtime, which Windows 11 and up-to-date Windows 10 already include; it still keeps its settings in `%APPDATA%`.

The builds on the releases page are not signed with a developer certificate yet, so the system warns on first launch:

- **macOS**: after moving ZShell to Applications, open it once; when macOS refuses, go to System Settings → Privacy & Security and click "Open Anyway". Alternatively, remove the quarantine flag in Terminal:

  ```bash
  xattr -dr com.apple.quarantine /Applications/ZShell.app
  ```

- **Windows**: when SmartScreen shows "Windows protected your PC", click "More info", then "Run anyway".

## Getting started

Add a session with **+** at the top of the sidebar, or import your SSH config from the menu next to it, then double-click the session to connect. To connect without saving a session, type `user@host[:port]` or `telnet host[:port]` in the search box and press Enter. Host keys, passwords and passphrases are asked in the terminal, as `ssh` does; after a disconnection, press Enter to reconnect. Right-click terminals, tabs and sessions for more, and see Settings → Keyboard Shortcuts for the keys.

Sessions and settings are saved in `~/Library/Application Support/ZShell/` on macOS, `%APPDATA%\ZShell\` on Windows and `~/.config/ZShell/` on Linux. Saved passwords go to the macOS Keychain, the Windows Credential Manager or the system keyring (GNOME Keyring, KWallet), and host keys to `~/.ssh/known_hosts`, shared with OpenSSH. See [PRIVACY.md](PRIVACY.md) for the details.

## Development

Requirements: Rust (stable), Node.js and pnpm; on Linux, also the system libraries listed in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) (in Chinese).

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
