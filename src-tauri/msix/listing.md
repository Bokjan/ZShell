# Microsoft Store listing

The text of ZShell's Store listing (English, United States), to paste into Partner Center: the
fields of "Store listings" in order, then the other pages that take text. Limits are Partner
Center's. Kept in step with the README's features; how to submit is in docs/DEVELOPMENT.md.

## Description

(10,000 characters at most)

ZShell is an SSH, Telnet and serial terminal for Windows, macOS and Linux, made for people who work on many servers and network devices every day.

One connection per tab: the terminal, the SFTP file panel and port forwarding share a single SSH connection, and duplicated tabs and split panes reuse it, so you authenticate once.

SSH
• Password, private key, SSH agent (the Windows OpenSSH agent and Pageant) and keyboard-interactive authentication, or all of them in OpenSSH's order.
• Host keys checked against ~/.ssh/known_hosts, shared with OpenSSH, and asked in the terminal as ssh does.
• Jump hosts, SOCKS5 and HTTP proxies or proxy commands, keepalives and automatic reconnection.
• Hosts imported from ~/.ssh/config.

Telnet and serial consoles
• For network devices and older systems, with saved logins typed at the prompts.
• Serial sessions reconnect when a USB adapter is plugged back in.

Files
• An SFTP panel with drag and drop in and out, transfers with progress, and remote files edited in your own editor and uploaded on save.
• rz and sz (ZMODEM) for hosts without SFTP.

Port forwarding
• Local, remote and dynamic SOCKS rules (-L, -R, -D) saved with the session, with live status.

Sessions, tabs and terminals
• Nested folders, search and quick connect, export and import.
• Per-session character encodings (GBK, Big5, Shift_JIS…), colors, fonts, commands run after login and environment variables.
• Local terminals (PowerShell), split panes, tabs in the title bar, several windows that tabs move between with their sessions, a full screen showing only the terminals, search, color schemes, copy on select, right-click paste and session logs.
• Quick command buttons and a command palette, and a compose bar that sends to several terminals or syncs what you type.

Security
• Sessions and saved passwords encrypted with a master password or a key in the Windows Credential Manager.
• Exports protected with a passphrase.
• An app lock that covers the window with Windows Hello after a period of inactivity, when the computer locks, or on demand.

Languages: English, Simplified Chinese, Japanese, German and French.

ZShell runs entirely on your computer: no account, no telemetry, nothing collected. It is free, with no ads or in-app purchases.

## What's new in this version

(1,500 characters at most; the changes since the version last submitted to the Store, from CHANGELOG.md, shortened if needed)

## Product features

(20 at most, 200 characters each)

1. One SSH connection per tab, shared by the terminal, the SFTP file panel and port forwarding
2. Password, private key, SSH agent and keyboard-interactive authentication, in OpenSSH's order
3. Jump hosts, SOCKS5 and HTTP proxies, proxy commands, keepalives and automatic reconnection
4. Import hosts from ~/.ssh/config; host keys shared with OpenSSH's known_hosts
5. Telnet and serial consoles for network devices
6. SFTP file panel with drag and drop, and remote files edited in your own editor
7. ZMODEM (rz and sz) file transfers
8. Local, remote and dynamic SOCKS port forwarding with live status
9. Session folders, search, quick connect, export and import
10. Per-session character encodings, colors, fonts, login commands and environment variables
11. Local PowerShell terminals, split panes and several windows
12. Quick commands, a command palette and a compose bar that sends to several terminals
13. Session logs
14. Sessions and passwords encrypted with a master password or the Windows Credential Manager
15. App lock with Windows Hello
16. In English, Simplified Chinese, Japanese, German and French

## Screenshots

(1 to 10; 1366 × 768 or larger, PNG; each with a caption of 200 characters at most)

## Additional information

- **Short title:** ZShell
- **Short description** (1,000 characters at most): SSH, Telnet and serial terminal with an SFTP file panel, port forwarding, saved sessions and split panes. One SSH connection per tab serves the terminal, the files and the forwards.
- **Search terms** (7 at most, 30 characters each, 21 words in all): SSH client; SFTP; Telnet; serial terminal; port forwarding; terminal emulator; ZMODEM
- **Copyright and trademark info** (200 characters at most): Copyright © 2026 Boyin Chen. All rights reserved.
- **Additional license terms** (10,000 characters at most): https://github.com/Bokjan/ZShell/blob/main/LICENSE.md
- **Developed by:** Boyin Chen
- **Minimum hardware:** none beyond Windows 10 version 1809 on x64 or ARM64 (set by the package).

## Properties

- **Category:** Developer tools
- **Privacy policy URL:** https://github.com/Bokjan/ZShell/blob/main/PRIVACY.md
- **Website:** https://github.com/Bokjan/ZShell
- **Support contact info:** https://github.com/Bokjan/ZShell/issues

## Notes for certification

ZShell is a terminal for SSH, Telnet and serial connections. No account or sign-in is needed. To try it without a server, click the + at the top of the tab bar to open a local PowerShell terminal; to try SSH, double-click a saved session or type user@host in the search box at the top of the session list and press Enter, then answer the prompts in the terminal.

runFullTrust: ZShell is a full desktop application. It starts local shells (PowerShell through ConPTY), reaches SSH agents through the Windows OpenSSH agent's named pipe and Pageant's window messages, opens serial ports, listens on local ports for port forwarding, and opens remote files in the editor the user chooses. None of this is possible in the app container.
