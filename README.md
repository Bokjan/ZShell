<img src="src-tauri/icons/128x128.png" width="96" alt="">

# ZShell

A cross-platform (macOS / Windows) SSH client inspired by Xshell, built with Tauri 2, Rust and xterm.js.

## Features

- **SSH sessions**: password, private key, SSH agent and keyboard-interactive (two-factor) authentication, plus an automatic mode that tries them in OpenSSH's order. Host keys are checked against `~/.ssh/known_hosts`, and saved passwords are kept in the system keychain.
- **One connection per tab**: the terminal, the file panel and port forwarding share a single SSH connection, so you authenticate once.
- **Jump hosts**: connect through one or more other sessions, like OpenSSH's `ProxyJump`.
- **Stays connected**: keepalives detect dead connections, and lost connections are re-established automatically.
- **SFTP file panel**: browse, upload and download with progress, drop files to upload, rename, delete, create folders and change permissions.
- **Port forwarding**: local (`-L`), remote (`-R`) and dynamic SOCKS (`-D`) rules, saved with the session, optionally started on connect, with live status.
- **Import from `~/.ssh/config`**: hosts, users, ports, keys, jump hosts and forwards.
- **Local terminals**: your login shell on macOS, PowerShell on Windows.
- **Terminal**: search with regular expressions, built-in color schemes, custom fonts, light and dark appearance.

Planned next: clipboard and context menus, tab management, ZMODEM (`rz` / `sz`), session folders and search. See the [roadmap](docs/ROADMAP.md) (in Chinese) and the [changelog](CHANGELOG.md).

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

Add a session with **+** in the sidebar, or import hosts from your SSH config, then click a session to open it in a new tab. Host key confirmations, passwords and passphrases are asked in the terminal, as `ssh` does. After a disconnection, press Enter in the terminal to reconnect.

| Action | macOS | Windows |
|---|---|---|
| Settings | ⌘, | Ctrl+, |
| Find in terminal | ⌘F | Ctrl+Shift+F |
| Show/hide the file panel | ⇧⌘E | Ctrl+Shift+E |
| Show/hide the port forwarding panel | ⇧⌘P | Ctrl+Shift+P |

### Where data is stored

- Sessions and settings: `~/Library/Application Support/org.boyin.zshell/` on macOS, `%APPDATA%\org.boyin.zshell\` on Windows (`profiles.json` and `settings.json`).
- Passwords and passphrases: the macOS Keychain or the Windows Credential Manager, never in those files.
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
