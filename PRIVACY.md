# Privacy Policy

Last updated: 2026-10-10

ZShell is an SSH, Telnet and serial terminal that runs entirely on your computer. It does not collect, transmit or share any personal data: there is no telemetry, analytics, crash reporting, advertising, user account or update check, and the developer receives nothing from your use of the app.

## Network connections

ZShell connects only to the servers you tell it to: the SSH and Telnet hosts you open, the jump hosts and proxies they go through, and the destinations of your port forwarding rules. A proxy command you set up for a session is run on your computer when that session connects. Serial sessions open only the serial device you choose, and the list of serial ports is read from the system when you pick one. Port forwarding rules you set up also listen on the local ports you choose. Links you click in the terminal open in your default browser.

## Data stored on your computer

Everything ZShell saves stays on your computer:

- **Sessions, settings, quick commands and log settings** are saved as JSON files in the app's configuration folder: `~/Library/Application Support/org.boyin.zshell` on macOS, and `%APPDATA%\org.boyin.zshell` on Windows. Sessions include host names, user names, ports, serial device names and settings, paths to private keys, port forwarding rules and the other options you set. Proxies are saved there too, with their addresses, user names and commands. If one of these files can't be read when ZShell starts, it is kept in the same folder under a name ending in `.bad-` and the date, and ZShell tells you about it.
- **Saved passwords** (for SSH, for typing at a Telnet server's password prompt, and for proxies) are kept only in the system's credential store, never in a file: in the macOS Keychain as items with the service `org.boyin.zshell`, and in the Windows Credential Manager as generic credentials whose names end in `.org.boyin.zshell`. Passphrases for private keys are not saved.
- **Host keys** of the servers you accept are added to `~/.ssh/known_hosts`, the file OpenSSH uses. When you remove host keys in the settings, ZShell rewrites that file and keeps its previous contents in `~/.ssh/known_hosts.old`, as `ssh-keygen -R` does. To check a server's key, ZShell also reads the other host key files OpenSSH reads (`~/.ssh/known_hosts2`, and `ssh_known_hosts` and `ssh_known_hosts2` in `/etc/ssh`, or `%ProgramData%\ssh` on Windows) but never writes to them. ZShell also reads `~/.ssh/config` when you import from it, and the private keys you choose for authentication.
- **Session logs** are recorded only when you turn them on, in the folder shown in the settings (`Documents/ZShellLogs` by default), which is created when the first log is written.
- **Exported sessions** are written to the file you choose, with your proxies, and never include passwords.
- **Temporary files** are created in the system's temporary folder while you edit a remote file in a local editor or drag files out of the file panel.
- **Interface state**, such as panel widths and the last download folder, is kept in the app's web view storage.

The clipboard is read only when you paste, and written only when you copy.

## Deleting your data

Uninstalling ZShell from a downloaded installer leaves your data in place. To remove it, delete the configuration folder above (and, on Windows, `%LOCALAPPDATA%\org.boyin.zshell`), remove the saved passwords (the `org.boyin.zshell` items in the Keychain, or the generic credentials ending in `.org.boyin.zshell` in the Credential Manager), and delete any session logs and exported files you want gone. Uninstalling the Microsoft Store version removes the configuration it created.

Entries added to `~/.ssh/known_hosts` are shared with OpenSSH and are not removed automatically; you can remove them in the settings under Known Hosts.

## Third parties

If you install ZShell from the Microsoft Store, Microsoft handles the download, installation and updates under its own privacy statement. The system web view that displays the app's interface (WebView2 on Windows, WebKit on macOS) is part of the operating system and follows the operating system's privacy settings. ZShell does not send them any data of its own.

## Changes and contact

Changes to this policy are published in this file, and its history is available in the repository. Questions can be raised in the [GitHub issues](https://github.com/Bokjan/ZShell/issues).
