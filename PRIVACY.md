# Privacy Policy

Last updated: 2026-10-14

ZShell is an SSH, Telnet and serial terminal that runs entirely on your computer. It does not collect, transmit or share any personal data: there is no telemetry, analytics, crash reporting, advertising or user account, and the developer receives nothing from your use of the app. The only connection ZShell makes on its own is the update check described below, which you can turn off.

## Network connections

ZShell connects only to the servers you tell it to: the SSH and Telnet hosts you open, the jump hosts and proxies they go through, and the destinations of your port forwarding rules. A proxy command you set up for a session is run on your computer when that session connects. Serial sessions open only the serial device you choose, and the list of serial ports is read from the system when you pick one. Port forwarding rules you set up also listen on the local ports you choose. Links you click in the terminal open in your default browser.

Unless you turn it off under Settings › About, ZShell checks for a newer version once a day, and whenever you choose Check for Updates: it asks GitHub (`api.github.com`) for the version number of ZShell's latest release. The request carries only your IP address, as any connection does, and `ZShell` as its user agent: not your version of ZShell, an identifier, or anything about your computer or sessions. It goes through the system's proxy if one is set. A newer version is only shown; you download it from its page in your browser if you choose to. The Microsoft Store version never checks, since the Store updates it.

## Data stored on your computer

Everything ZShell saves stays on your computer:

- **Sessions, settings, quick commands and log settings** are saved as files in the app's configuration folder: `~/Library/Application Support/ZShell` on macOS, `%APPDATA%\ZShell` on Windows, and `~/.config/ZShell` on Linux. Sessions include host names, user names, ports, serial device names and settings, paths to private keys, port forwarding rules, your notes and the other options you set. Proxies are saved there too, with their addresses, user names and commands. If the settings or the session log index can't be read when ZShell starts, the file is kept in the same folder under a name ending in `.bad-` and the date, and ZShell tells you about it.
- **Protection of sessions and passwords** is your choice, under Settings › Security. Your sessions, folders, proxies, quick commands and saved passwords are kept together in `vault.json` in the configuration folder. Encrypted, they are encrypted with AES-256-GCM, and the key is protected by your master password, which is never saved, or kept in the system's credential store (also when you ask ZShell to remember your master password). Not encrypted, the file holds them as they are, passwords included. When ZShell starts for the first time it encrypts them with a key in the system's credential store, or, if there is none, doesn't encrypt them. Settings, the session log index and session logs are never encrypted. The credential store is the macOS Keychain (items with the service `org.boyin.zshell`), the Windows Credential Manager (generic credentials whose names end in `.org.boyin.zshell`), or on Linux the system keyring through the Secret Service, such as GNOME Keyring or KWallet (items with the service `org.boyin.zshell`); the key is the entry whose name begins with `vault-key:`. On macOS and Linux the configuration files can only be read by your user account.
- **The app lock**, when you turn it on under Settings › Security, unlocks with the system's own authentication: Touch ID or your login password on macOS, Windows Hello on Windows, and on Linux the desktop's authentication dialog (your account password, or a fingerprint where set up), through polkit. ZShell only learns whether it succeeded; your fingerprint, face, PIN or password never reach it. To lock when the computer is idle, locks or sleeps, ZShell asks the system how long it has been idle and listens for it locking and sleeping (on Linux, from GNOME's Mutter and from logind). Its settings are kept with your sessions, in `vault.json`.
- **Saved passwords** are those for SSH, for typing at a Telnet server's password prompt, and for proxies, kept as described above. Passphrases for private keys are not saved.
- **Host keys** of the servers you accept are added to `~/.ssh/known_hosts`, the file OpenSSH uses. When you remove host keys in the settings, ZShell rewrites that file and keeps its previous contents in `~/.ssh/known_hosts.old`, as `ssh-keygen -R` does. To check a server's key, ZShell also reads the other host key files OpenSSH reads (`~/.ssh/known_hosts2`, and `ssh_known_hosts` and `ssh_known_hosts2` in `/etc/ssh`, or `%ProgramData%\ssh` on Windows) but never writes to them. ZShell also reads `~/.ssh/config` when you import from it, and the private keys you choose for authentication.
- **Session logs** are recorded only when you turn them on, in the folder shown in the settings (`Documents/ZShellLogs` by default), which is created when the first log is written.
- **Exported sessions** are written to the file you choose, with your proxies. Saved passwords are included only if you ask, and then the whole file is encrypted with a passphrase you give for it, which is not saved; otherwise the file is not encrypted.
- **Temporary files** are created in the system's temporary folder (on Linux, `~/.cache/org.boyin.zshell`, which only your user account can read) while you edit a remote file in a local editor or drag files out of the file panel.
- **The update check** keeps the time of the last check and the latest version it found in `update.json` in the configuration folder. Whether to check, and a version you chose to skip, are saved with the settings.
- **Interface state**, such as panel widths and the last download folder, is kept in the app's web view storage (on Linux in `~/.local/share/org.boyin.zshell`).

The clipboard is read only when you paste, and written only when you copy.

## Deleting your data

Uninstalling ZShell from a downloaded installer leaves your data in place. To remove it, delete the configuration folder above (and, on Windows, `%LOCALAPPDATA%\org.boyin.zshell`; on Linux, `~/.cache/org.boyin.zshell` and `~/.local/share/org.boyin.zshell`), remove the key (the `org.boyin.zshell` items in the Keychain or the system keyring, or the generic credentials ending in `.org.boyin.zshell` in the Credential Manager), and delete any session logs and exported files you want gone. Uninstalling the Microsoft Store version removes the configuration it created.

Entries added to `~/.ssh/known_hosts` are shared with OpenSSH and are not removed automatically; you can remove them in the settings under Known Hosts.

## Third parties

If you install ZShell from the Microsoft Store, Microsoft handles the download, installation and updates under its own privacy statement. Otherwise the update check above reaches GitHub, which hosts ZShell's releases, under [GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement). The ZShell product website is hosted on GitHub Pages; visiting it sends GitHub the information normally included in a web request, such as your IP address and browser headers. The website has no analytics, advertising or other third-party scripts. The system web view that displays the app's interface (WebView2 on Windows, WebKit on macOS) is part of the operating system and follows the operating system's privacy settings. ZShell does not send them any data of its own.

## Changes and contact

Changes to this policy are published in this file, and its history is available in the repository. Questions can be raised in the [GitHub issues](https://github.com/Bokjan/ZShell/issues).
