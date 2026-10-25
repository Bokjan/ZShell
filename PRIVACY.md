# Privacy Policy

Last updated: 2026-10-25

ZShell is an SSH, Telnet and serial terminal that runs entirely on your computer. It does not collect, transmit or share any personal data: there is no telemetry, analytics, crash reporting, advertising or user account, and the developer receives nothing from your use of the app. The only connection ZShell makes on its own is the update check described below, which you can turn off.

## Network connections

ZShell connects only where you tell it to: the SSH and Telnet hosts you open, the jump hosts and proxies they go through, and the destinations of your port forwarding rules, which also listen on the local ports you choose. A proxy command you set up runs on your computer when its session connects. Serial sessions open only the device you choose. Links you click in the terminal open in your default browser.

Unless you turn it off under Settings › About, ZShell asks GitHub (`api.github.com`) for the version number of its latest release once a day, and when you choose Check for Updates. The request carries only your IP address, as any connection does, and `ZShell` as its user agent, and goes through the system's proxy if one is set. A newer version is only shown; you download it yourself if you choose to. The Microsoft Store version never checks, since the Store updates it.

## Data stored on your computer

Everything ZShell saves stays on your computer, in its configuration folder unless said otherwise: `~/Library/Application Support/ZShell` on macOS, `%APPDATA%\ZShell` on Windows, and `~/.config/ZShell` on Linux. On macOS and Linux only your user account can read it.

- **Sessions, folders, proxies, quick commands, saved passwords and the app lock's settings** are kept in `vault.json`. Sessions include host names, user names, serial settings, paths to private keys, port forwarding rules and your notes; saved passwords are those for SSH, Telnet logins and proxies, and passphrases for private keys are never saved. Under Settings › Security you choose how the file is protected: encrypted with AES-256-GCM, with the key kept in the system's credential store or protected by a master password, which is never saved (unless you ask ZShell to remember it, which keeps the key in the credential store); or not encrypted, passwords included. The first start uses the credential store if there is one. Resetting your sessions on the unlock screen keeps the previous file beside the new one, renamed with the date, along with its key.
- **Settings**, the session log index and the update check's state (the time of the last check and the latest version found, in `update.json`) are not encrypted. A settings or index file that can't be read is kept under a name ending in `.bad-` and the date, and ZShell tells you about it.
- **The app lock** unlocks with the system's own authentication: Touch ID or your login password on macOS, Windows Hello on Windows, and the desktop's authentication dialog through polkit on Linux. ZShell only learns whether it succeeded. To lock when the computer is idle, locks or sleeps, it asks the system how long it has been idle and listens for it locking and sleeping.
- **Host keys** you accept are added to `~/.ssh/known_hosts`, as OpenSSH does (with the host name hashed if `HashKnownHosts` is on in your `ssh_config`); removing them in the settings keeps the previous file as `known_hosts.old`. ZShell also reads the other host key files OpenSSH reads, `~/.ssh/config` and the system's `ssh_config` when you import from it or add a host key, and the private keys you choose.
- **Session logs** are recorded only when you turn them on, in the folder shown in the settings (`Documents/ZShellLogs` by default).
- **Exported sessions** are written to the file you choose. Saved passwords are included only if you ask, and then the whole file is encrypted with a passphrase you give, which is not saved.
- **Temporary files** are created in the system's temporary folder (on Linux, `~/.cache/org.boyin.zshell`) while you edit a remote file in a local editor or drag files out of the file panel.
- **Crash log**: if ZShell stops because of an internal error, a line saying when, which version, where in its code and the error's message is added to `crash.log`. It stays on your computer and is never sent; you can delete it at any time.
- **Interface state**, such as panel widths and recently opened sessions, is kept with the web view's cache in its storage (see below).

The clipboard is read only when you paste, and written only when you copy.

## Deleting your data

Uninstalling ZShell from a downloaded installer leaves your data in place. To remove it, delete:

- the configuration folder above;
- the web view's storage: `~/Library/WebKit/org.boyin.zshell` and `~/Library/Caches/org.boyin.zshell` on macOS, `%LOCALAPPDATA%\org.boyin.zshell` on Windows, `~/.local/share/org.boyin.zshell` and `~/.cache/org.boyin.zshell` on Linux;
- the key: the items with the service `org.boyin.zshell` in the macOS Keychain or the Linux keyring (GNOME Keyring, KWallet), or the generic credentials whose names end in `.org.boyin.zshell` in the Windows Credential Manager;
- any session logs and exported files you want gone.

Uninstalling the Microsoft Store version removes the configuration it created. Entries in `~/.ssh/known_hosts` are shared with OpenSSH and stay; you can remove them in the settings under Known Hosts.

## Third parties

If you install ZShell from the Microsoft Store, Microsoft handles the download, installation and updates under its own privacy statement. The update check reaches GitHub, which hosts ZShell's releases, under [GitHub's privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement). The product website is hosted on GitHub Pages, which receives what any web request carries, such as your IP address and browser headers; the website has no analytics, advertising or other third-party scripts. The system web view that displays the interface (WebView2 on Windows, WebKit on macOS, WebKitGTK on Linux) is part of the operating system and follows its privacy settings; ZShell sends it no data of its own.

## Changes and contact

Changes to this policy are published in this file, and its history is available in the repository. Questions can be raised in the [GitHub issues](https://github.com/Bokjan/ZShell/issues).
