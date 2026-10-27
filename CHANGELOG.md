# Changelog

Changes in each release, generated from the commit history with [git-cliff](https://git-cliff.org): what changed for users, then internal restructuring.

## [3.16.0] - 2026-10-27

### Features

- The file panel goes back and forward through the folders it showed, including those it went to while following the terminal: with its new back and forward buttons, the mouse's side buttons, Cmd+[ / Cmd+] on macOS or Alt+Left / Alt+Right elsewhere. A folder that turns out to be gone is left out of the history, and listing a folder that doesn't exist says so. Its toolbar is rearranged: back, forward, up, refresh and the path on the first row; upload, new folder, the filter, hidden files and the terminal's folder on the second.
- Letter and punctuation shortcuts are on the keys labeled so on every keyboard layout (AZERTY, QWERTZ, Dvorak, Japanese), as in other apps; where a layout types no Latin letters (Russian), they stay where they are on a US keyboard. The macOS menu bar no longer moves its shortcuts to other keys on some layouts. Outside macOS, panes are also split with Ctrl+Shift+S (right) and Ctrl+Shift+D (down); Alt+Shift+= and Alt+Shift+- still work.

### Bug Fixes

- Splitting a pane or duplicating a tab on a connection whose file panel had been opened no longer freezes the app. Introduced in 3.14.0.
- On Windows and Linux, dragging files out of the window while a terminal on the same connection prints a lot no longer freezes every window for up to two minutes and fails the drop.
- A server that keeps the connection up but never answers opening an SFTP channel no longer leaves the file panel and transfers on that connection waiting for good: after 30 seconds of waiting for the server, the operation fails and the next one tries again. Channels given up on are closed rather than left open at the server.
- On Windows, while the title bar's window menu (a right click or Alt+Space) is open, typing, resizing and busy terminals in every window no longer wait for it to close.
- A session log no longer silently stops being written after two reconnections while the previous connection was still finishing writing it, to a slow or network drive for example; such a log also couldn't be cleaned up or deleted until a restart, and quitting waited for it. Introduced in 2.5.4.

### Internal

- Starting a session no longer runs its backend's first step while holding the lock that every keystroke and resize takes, so a fault there can no longer freeze every window.

## [3.15.0] - 2026-10-26

### Features

- While the terminal is scrolled back, output that arrives below no longer goes unnoticed: a button at the bottom of the pane counts the new lines and scrolls back to the end, and screen readers are told once. Lines a program draws over (a progress bar, a prompt being redrawn) aren't counted until it writes a new one. Once the scrollback is full and new output starts removing the lines being read, a notice at the top of the pane says so and where to keep more lines.
- When an internal error ends the app, a line about it (the time, the version and system, the thread, where and the message) is added to `crash.log` in the configuration folder, so there is a record of why it closed, also on Windows. The file starts again past 64 KiB, and the privacy policy says so.

### Bug Fixes

- A password, passphrase or verification code prompt, or a wait for a Duo push, no longer keeps a CPU core busy until it is answered. The update of russh this comes with also fixes its handling of unbounded EXT_INFO messages and of a peer declaring a zero maximum packet size.
- A click outside a dialog no longer answers a confirmation (overwriting or deleting files, deleting sessions, closing a window) with Cancel or throws away what was typed in a form; as in native dialogs, it closes only the command palette, the settings, the known hosts and the licenses. While a dialog saves or applies something, it can't be closed or cancelled, so its outcome or error is never lost.
- The terminal's scroll bar no longer stays shown while the pointer is anywhere over the terminal: it shows while the view scrolls, while the pointer is on its strip or while its slider is dragged, and fades out half a second after scrolling stops. Output arriving below the lines being read back doesn't show it either.
- Cancelling a transfer takes effect at once even while it waits behind a large download on the same connection, lists folders or creates an upload's folders. A cancelled download no longer leaves folders open at the server for as long as the connection lasts.
- A large file always leaves one of the connection's transfer slots to small files, also with several large files waiting, so a drag or a file opened in an editor no longer waits for large downloads to end.
- A folder of files dragged out of the window shows as fully done once its files have arrived, instead of sometimes staying at "0 of 300 files".
- On Windows 10, making a local terminal narrower no longer shows lines twice or garbled: the terminal is told which Windows build its ConPTY comes with.
- With a long scrollback, a terminal's columns follow a resize once it pauses also while a full-screen program such as vim, tmux or less runs.
- A window opened after installing a font that was missing from the font list forms that font's ligatures, rather than those of the next font in the list, without restarting the app.
- Splitters for panes, the side panel and the sidebar stop following the pointer when the mouse button is released outside the window, instead of going on until the next click while the terminals ignored the mouse.
- Holding a number field's arrow and then switching apps from the keyboard no longer keeps stepping to the end of its range.
- A forwarding rule's auto-start tag sits under its kind, cut short with an ellipsis and explained in its tooltip, so the mappings stay lined up and every rule has the same height in every language.

### Performance

- Downloading many items is much faster: those dragged out of the window are downloaded together rather than one by one (500 small files from a distant server took minutes), and the items chosen are looked up at the server all at once rather than twice each, one after another, which made 500 files wait over a minute before the first byte. Deleting what an upload created removes a level of folders at a time.
- Resizing the window or a splitter no longer rewraps a long scrollback on every frame: the rows follow at once, and the columns of a terminal with more than 200 lines once the resizing pauses, as in VS Code.
- Moving a tab to another window no longer freezes both windows for a moment with a long scrollback: its screen is handed over through the backend instead of in an event.
- Pasting a few MiB into a local shell or serial line no longer copies gigabytes of memory, and Telnet passes on up to 64 KiB of output at once, like the other connections.
- Long lists render less: the file panel and session list no longer render every row on each progress update, keystroke in the search field or move of the sidebar's edge, rows no longer each listen for the language, and a terminal no longer renders again whenever its title or progress changes.
- Work that a network drive could hold up no longer stalls other sessions or tabs: finding where a downloaded file goes and a free name for it, looking at session log files, and clearing the folders of earlier drops when a drag out of the window starts. Importing sessions no longer holds up saving or unlocking in other windows.
- The app does less while idle: the traffic lights' text is worked out every 10 seconds while nothing moves, the app lock checks the idle time once a minute while it can't lock, a session log's writer sleeps while there is nothing to write, a closed tab's session no longer sends output to the window, and windows opened later reuse the font's ligature rules.

### Internal

- Large modules are split into parts that stand on their own: SFTP uploads and downloads, how a transfer ends and what its undo deletes, the SSH module's tests, and how a pane follows its session. The terminal view's main effect is split into parts that each set up and clean up their own, and components and hooks no longer depend on the app's flows, which a test checks.

## [3.14.0] - 2026-10-25

### Features

- Activity lights in the file and forwarding panels. The file panel shows lights for its browsing channel beside the upload buttons, and lights with the transfer rate in the transfer list's header; the forwarding panel shows the rate of all running rules in its header, and of each rule under its status. Their tooltips, like the status bar's, name the SSH connection and the channels the traffic goes through. Rates follow traffic within a second of it starting, keep their place as the numbers change, and lights in a hidden panel stop working until it is shown again.
- Uploads and downloads can be undone: once a transfer ends, done, failed or cancelled, a trash button in its row permanently deletes the files and folders it created, after a confirmation. Files it wrote over, and files changed since it created them, are kept and counted, so nothing that was there before is lost; what an upload created is deleted only on the server it went to. A cancelled or failed download can also be shown in Finder / File Explorer.
- ZShell's own messages in the terminal (connecting, how a session ended, a lost connection, reconnecting, forwarding and ZMODEM) start each line with the local time they were printed. The login prompts, echoed input and a proxy command's errors are printed as before, and a session log that already starts its lines with the time doesn't get it twice.
- Settings › Session Logs has a time format for log lines and the terminal's messages: date and time, or the time alone, with or without milliseconds, or a format of your own with %Y, %y, %m, %d, %H, %M, %S, %3f and %%. The field shows an example of the current time, or why a format can't be used. A log line's time is now when the line started rather than when it ended.

### Bug Fixes

- On Windows, pressing F11 in a maximized window no longer leaves a black strip where the taskbar was.
- The web view no longer shows its own tooltip beside the app's for the element around the one under the pointer, such as a forwarding rule's explanation over its lights.
- Field titles in dialogs and settings are set apart from the hints below them, in the text's color and semibold, and the hint that belongs to a field is read with it by screen readers.
- The settings put the title and the close button in a row of their own, with the same margins on both sides as the other dialogs.
- With the session list hidden, the hint in an empty window names the shortcut that shows the list instead of pointing to the left.
- A cancelled transfer tells how many files it finished, and the progress line keeps the file name and amounts still as the numbers grow.
- The transfer list's title is shortened in a narrow panel instead of running under "Clear Finished", and a transfer of several items is named in the current language after the language changes.
- A forwarding rule's connection count moves to the next line as a whole in a narrow panel instead of breaking mid-word.
- Buttons that read as links look the same everywhere, without an underline on hover.
- The third-party notices list only what the app is built from, leaving out about 100 crates used only while compiling.


## [3.13.1] - 2026-10-23

### Bug Fixes

- Updated the ctutils library used by the SSH cryptography, which could take a different time depending on secret values in some operations (RUSTSEC-2026-0385).

## [3.13.0] - 2026-10-23

### Features

- The session list can be hidden, keeping the tabs and panels: with the button beside the app menu's button (beside the traffic lights on macOS), which stays in place and moves to the start of the tab bar while the list is hidden, with View › Hide / Show Session List in the app menu and the macOS menu bar, or with ⌃⌘S on macOS and Ctrl+Shift+B elsewhere. Searching or creating a session shows the list again, scrolled to the selected row, and whether it is hidden is kept per machine, like its width.
- The terminal uses the whole width for text: the scroll bar, which fades out when unused, is drawn over the last column rather than keeping a strip free beside it, and is narrower.

### Bug Fixes

- Ctrl+, (⌘, on macOS) closes the settings as well as opening them, unless another dialog is open over them.

### Internal

- The bundled license notices store each paragraph once, which makes them less than half the size; every package shows the same text as before.

## [3.12.1] - 2026-10-23

### Bug Fixes

- A port forwarding rule stopped as a shell exits, a tab closes or a connection is lost could, rarely, be taken for one that had failed: the forwarding panels went on showing it active, and it wasn't moved to another tab of the session, started again on reconnecting or started again with another shell.
- A tab reconnecting at the very moment its connection was lost could start without the forwarding rules that had been running.

## [3.12.0] - 2026-10-23

### Features

- The computer can be kept from sleeping while sessions are connected, chosen in Settings › Power › Prevent sleep while connected: never (the default), when plugged in, or always. While an SSH, Telnet or serial session is connected, connecting or waiting to reconnect, inactivity no longer puts the computer to sleep, so the sessions stay connected; the display still turns off and the screen still locks, and closing the lid or choosing Sleep still puts it to sleep. On Windows computers with Modern Standby, Windows still sleeps on battery 5 minutes after its sleep time, as the settings say.
- When the shell of a connection's last tab exits, connections already made through its port forwarding are no longer cut. As in OpenSSH, the tab stays open, says which forwarding it waits for and shows how many connections are left, and closes once they have ended; Enter starts a new shell on the same connection, Ctrl+C disconnects them at once and Ctrl+D closes the tab. The forwarding stops listening, and moves to another connected tab of the same session if there is one. Closing a tab whose forwarding carries connections asks first, saying how many it disconnects.
- Deleting a session stops the port forwarding running in its connected tabs, which before kept their ports open with no way left to see or stop them. When any runs, the confirmation names the tabs using it, in other windows too.
- New host keys are recorded hashed in known_hosts when ssh_config's `HashKnownHosts` is on for the host, as OpenSSH does (the system ssh_config of Debian and Ubuntu turns it on), rather than always with the host name in clear.

### Bug Fixes

- A connection that died while the computer slept or the network changed is noticed within seconds of typing, browsing or transferring with SFTP, a forward's client sending data or opening a connection, or the system waking, and reconnects as usual. Before, the tab looked connected while nothing worked for minutes, until keepalives gave up, or for good when the system couldn't send what was written. A busy connection or one on a slow link is not taken for a dead one, and sessions with keepalives turned off are not checked.
- Automatic reconnection after a lost connection starts the forwarding rules that were running again, as reconnecting by hand did.
- A forwarding rule's connection gives up after 30 seconds when the server hasn't connected to the target, and the rule's state says that the server took too long; before it could wait minutes, showing nothing wrong. Connecting through a jump host that can't reach the next hop gives up after 30 seconds too.
- A forwarding rule on `*` listens on IPv6 as well as IPv4, as in OpenSSH, and `*` in ssh_config stays `*` when imported rather than becoming `0.0.0.0`.
- A known_hosts file that can't be read (one left to root by sudo, for example) is reported as such, rather than as a failed handshake retried again and again by automatic reconnection.
- SFTP:
  - New files and folders get the permissions of those copied, as `sftp put` and `get` do, so an uploaded private key stays private and a script keeps its execute bit. Files written over keep their own.
  - Transfers on a connection share the limit of files copied at once, so several large downloads at once no longer time out on a slow link.
  - A downloaded file is written to the disk before it replaces the one it overwrites, so a power cut can't leave a partial file in its place.
  - File and folder names are sorted in the interface language rather than the system's.
- Resetting the vault after a forgotten master password keeps the old sessions file (renamed `vault.json.reset-<time>`) and its key, so it can be opened again if the password comes back to mind; the confirmation is now typed as RESET. Unencrypted vault and export files are checked after writing too, and encrypting leaves no stray copies of the sessions and passwords in memory.
- Importing from ssh_config checks the sessions and proxies as saving does, names the proxy already here that an imported session will use, and shows IPv6 hosts in brackets. Sessions files over 16 MB are refused, and scanning one whose sessions jump through each other is no longer slow.
- Quick commands and groups without an id in the vault keep the same one across restarts, so sessions' quick command groups keep finding them.
- Settings and quick commands saved by another window while this one was starting are no longer undone by its next change.
- A tab moved to another window and reconnected meanwhile no longer gets the old screen drawn over it, and a session offered to a new window that couldn't be created goes back to its tab.
- A font whose ligature tables describe more than can be read no longer ends the app when ligatures are taken from the font; it gets the common sequences instead.
- Telnet logs in with `?` for characters the session's encoding can't write, rather than HTML character references.
- A ZMODEM peer sending only `*` is given up on once past the limit of bytes skipped.
- Lists of names in messages are written as the interface language writes them ("a、b和c" in Chinese).
- On macOS, ⌘↩ toggles terminal full screen also while the terminal or the compose bar has the focus.
- On Windows, text fields show the focus in contrast themes.
- On Linux, terminals no longer start with another font for a moment when another window changed the settings during startup.

### Performance

- Opening local terminals and serial ports, setting the window's theme, creating downloads and looking up a local terminal's foreground program no longer hold up other work; on Windows the last froze the window while it ran.

### Internal

- App's flows (opening sessions, the session editor, logging, dialogs, update notices and quick commands) moved into hooks of their own, which read the settings they need themselves; the window's actions, with their shortcuts and app menu items, are declared in one table.
- Local and dynamic forwarding share their listening loop, and what changing or deleting a session does to its forwarding is decided in one place.
- The vault's debug output leaves out saved passwords and the master password.

## [3.11.0] - 2026-10-21

### Features

- The close tab shortcut (⌘W, Ctrl+Shift+W on Windows and Linux) closes the top dialog, as a click on its backdrop does: settings, the session editor and the other dialogs close, asking first where there are unsaved changes, and a dialog that must be answered stays open. Before, the shortcut did nothing while a dialog was open.

### Bug Fixes

- On Windows, F5, Ctrl+R and the web view's other browser shortcuts (Alt+Left to go back, Ctrl+P to print, Ctrl+F to find) no longer act on the window. F5 or Ctrl+R pressed outside the terminal, in the session list or the tab bar for example, reloaded the window and closed every session in it. Programs in the terminal still get these keys.
- On macOS with an input method on, shifted punctuation (the full-width ？ of Shift+/ in Chinese Pinyin, for example) is typed in the terminal on the first press rather than the second.

## [3.10.0] - 2026-10-20

### Features

- Password and passphrase fields have a button at their right end that shows or hides what is typed, the same on every platform (WebView2's own reveal button, which the other web views lack, is hidden). It works from the keyboard, and a disabled field always shows dots.

### Bug Fixes

- Drop-down lists are drawn in the app's own style and colors on every platform, rather than each platform's. The terminal type, serial device and baud rate fields have a button that lists every suggestion, where before a field showing `xterm-256color` offered nothing else. A list placed for a few suggestions moves and widens when it shows more, and a click on the label of an open drop-down closes it rather than opening it again. Typing the same letter again in a menu or a drop-down goes through the items that start with it.
- Checkboxes and radio buttons are drawn in the theme's colors, rather than in Chromium's blue on Windows, the system accent color on macOS or GTK's on Linux; Windows contrast themes keep the system's colors.
- The multi-line fields of dialogs (login commands, environment variables, quick commands, proxy commands and notes) grow with their text up to twelve lines, then scroll, instead of having a handle to drag, and fit their text again when the window or the text size changes.
- Empty fields in the port forwarding dialog are reported in the app's own words and language, rather than with the web view's bubble in the system's language.
- Screen readers no longer read the buttons inside text fields as part of the fields' names ("Password Show password", "Baud rate Increase Decrease").
- On Windows, the web view's own tooltip no longer shows alongside the app's when the pointer moves within an element, from its icon onto its text for example.
- On Windows, a window starting up shows the theme's background color rather than white until the page has painted.
- The session list no longer says there are no sessions while they are still being read at startup.

## [3.9.0] - 2026-10-19

### Features

- Programs in the terminal can do more, as in other modern terminals:
  - Copy to the clipboard (OSC 52), for example from tmux or Neovim on a server, up to a size set in Settings › Mouse and Clipboard (1024 KB by default). Programs can never read the clipboard, and copying can be turned off there.
  - Report their progress (OSC 9;4), shown as a ring on their tab and on the window's taskbar button, or on the Dock icon and the Linux launcher, combined across windows. It goes away when the session ends or after 30 seconds without news.
  - Send notifications (OSC 9, 777 and 99, as iTerm2, urxvt and kitty do), for example when a long command finishes. A tab in the background is marked, and while ZShell is in the background the system shows the notification, at most one every 3 seconds for each pane (the last of a burst rather than the first), cut to 4096 characters. While the app is locked the notification only says that a program sent one, without its text or the pane's title. Notifications can be turned off in Settings › Terminal.
  - Set the pointer's shape over the terminal (OSC 22), by its CSS or X11 name, kept separately for the main and the alternate screen as kitty does.

### Bug Fixes

- The Option key is capitalized in the Japanese interface.

### Internal

- The setting for notifications from programs is laid out with the same component as every other setting.

## [3.8.0] - 2026-10-19

### Features

- Terminals can draw programming ligatures, chosen in Settings › Terminal › Ligatures (off by default). Common sequences joins a fixed list of the usual operators (`->`, `!=`, `===` and the rest of Iosevka's set, as VS Code does), which a font with programming ligatures, such as Fira Code, JetBrains Mono or Cascadia Code, draws as one symbol. From the font joins exactly what the terminal's font joins, by the rules read from the installed font, and falls back to the common sequences where the font can't be read. While ligatures are off, terminals drawn without WebGL no longer show them either.

### Bug Fixes

- The custom background color is chosen with a color field of the app's own on every platform: a swatch that opens a popup with a saturation and brightness area, a hue bar, the color in hex and an eyedropper that picks a color from anywhere on the screen (on Windows also from the keyboard, with the arrow keys and Enter). The eyedropper of WebView2's color popup did nothing, GTK's had none, and the three popups looked different.

### Performance

- With more terminals visible than the web view has WebGL contexts for (about 16), terminals no longer take them from each other and fall back to slower drawing in turn: at most 12 draw with WebGL, and the least recently used of the rest draw without it until focused or until there is room again.
- Saving a file opened from the file panel in a local editor uploads it about 0.3 seconds after the editor writes it, rather than 0.5 to 1 second, and an idle edit no longer wakes the app. Starting or stopping an edit no longer holds up other sessions.
- Local shells and serial lines on macOS and Linux no longer use threads of their own, one of which woke five times a second even while idle, and a serial device is let go of as soon as its session ends.
- Folder listings are sorted once, by the file panel, instead of also in the backend.

### Internal

- The largest components are split, moving out the parts that stand on their own: the sidebar's rows, the terminal's ZMODEM handling and menu items, and the file panel's file operations and Upload button. Local shells and serial lines on Unix share their non-blocking reads and writes, and the SFTP transfer tests are in a file of their own.

## [3.7.0] - 2026-10-18

### Features

- Panes can move to another tab or window. The terminal's menu moves a pane to a tab of its own, next to the focused pane of another tab (right of it or below, wherever there is room), or to another window. A pane can also be dragged by its grip, which shows near its top edge: within its tab, onto the tab bar as a new tab, onto the panes of another tab, or out of the window. A tab dragged from the tab bar onto the panes of the tab shown joins them. A pane moving between tabs keeps its terminal, its screen, its side panels and their file transfers as they were.
- A tab or pane dragged to rest on another tab shows that tab after a moment, with a bar filling along the tab's bottom meanwhile, so that it can be dropped on that tab's panes. Unless it joins them or leaves the window, the tab shown before comes back.
- Tabs are dragged along the tab bar as in browsers: the dragged tab follows the pointer, the others slide over to make room, and once dropped it slides into its new place, or back into its old one after Escape.
- Menus taller than the window scroll, and typing the start of an item highlights it.

### Bug Fixes

- Commands, paths and keys in the interface (a session's login commands and environment, the compose bar, the command palette, paste previews, path lists, host keys, forwarding rules) are drawn in the terminal font set in Settings, falling back to one monospace font stack; on Linux, Latin letters stay fixed-width beside Han characters. The placeholders of command fields and of the compose bar, which are sentences, are in the interface font.
- An app font setting with a stray quote no longer replaces the whole interface font with the browser's default.

### Internal

- Terminals and side panels are drawn into elements that move with their pane rather than by the tab's page, so that a pane changes tabs without its terminal being mounted again. Dragging tabs and dragging panes share their mouse handling.

## [3.6.1] - 2026-10-17

### Bug Fixes

- Settings and quick commands changed in two windows at about the same time are both kept: a change still waiting to be saved in one window no longer overwrites what another window saved meanwhile. Forwarding rules are saved and deleted one at a time, so a rule added or changed in another window is no longer put back to this window's copy, and deleting a rule stops only that rule.
- A tab being moved to another window can no longer be closed halfway, which ended the sessions the other window was about to take. If that window closes or reloads before taking them, they stay in the tab here.
- A ZMODEM transfer starting just as its session moves to another window no longer waits forever for an answer from the old window.
- Session logs are written to the end when the app quits; up to a second of output was lost before.
- A reloaded window cancels its file transfers and stops watching the files it was editing, and only the window that started a transfer or an edit can cancel or upload it.
- A serial session keeps reading keystrokes while the device holds up output with flow control, so Ctrl+C still cancels a ZMODEM upload; SSH and Telnet sessions likewise keep reading input while a write waits.
- A connection lost while the shell or the SFTP channel was being started now says so, instead of reporting an unexpected error.
- The app lock's screen no longer comes off when an answer from before the app locked arrives late, and an unlock is no longer lost to a status read asked while Touch ID or Windows Hello was asking.
- Terminals open when the interface language changes describe their input and output to screen readers in the new language.
- macOS: a click on the file list of a window in the background brings that window forward once the button is released, as intended; the window lacked the permission to do it. Windows closed are no longer watched for full screen changes.
- Windows contrast themes show the states they dropped: the forwarding switch, selected path suggestions, toggled toolbar buttons, drop targets, pane dividers, progress bars and the marks on tabs now use system colors.
- Settings › Terminal says how much memory the scrollback takes (about 25 MB per 10,000 lines in a wide window, for every pane).
- Controls look the same everywhere: segmented controls (including the side panel switch) have rounded selected segments, focus rings and the drop outline are no longer cut off, badges, fields and buttons stay in proportion at the larger text sizes, and selected and drop target rows look alike in every list.
- The pages are granted only the permissions they use.

### Performance

- Terminal output reaches the window in far fewer messages (a local terminal on macOS sent about a thousand per MiB of `cat`), and is acknowledged in larger batches, so fast output takes much less of the main thread that every window shares.
- SFTP transfers of many small files are many times faster: up to eight small files are copied at once, folders are listed and created a level at a time, and deleting a folder removes its files several at a time (10,000 files from a server 100 ms away took over an hour to copy, and a quarter of an hour to delete). Uploads keep as many writes on the way as OpenSSH's `sftp`, and new files are no longer synced to the server's disk one by one.
- A folder with many files opens at once: from 500 entries the file panel draws only the rows in view, and the listing is sorted with less work.
- Resizing the window no longer renders every tab and fits every terminal on each frame; hidden tabs fit once shown, and the session hears of a new size at most every 100 ms.
- Busy terminals cost less: a program that keeps setting its title, or the same title at every prompt, no longer makes the whole window and the other windows render again, the session list draws only the rows that change, a hidden terminal isn't searched again as output arrives, and the status bar and tooltips do less every second and every key.
- Switching tabs keeps the terminals' glyph atlas instead of building it again, and a terminal that loses WebGL (after sleep or a GPU switch) tries it again a few times instead of staying on the slower renderer.
- Windows show sooner: the app no longer waits for Windows Hello or polkit at startup while the app lock is off, finds the default local shell (Windows) and the desktop's monospace font (Linux) without holding up the first window, and loads only the interface language in use.
- Saving settings and quick commands no longer blocks every window while the file is written.
- Text logs, ZMODEM detection and CRCs, Telnet's login prompt detection and forwarded connections do less work, and a file being edited is uploaded without reading it all into memory.

### Internal

- The types of what each command takes and returns, and the constants both sides share, are generated for the frontend instead of copied there.
- The vault answers queries from what its last change published, and each store changes its part from the data as it is.
- What a new session runs is resolved outside the commands; the window code has the same functions on every platform, with tests of what moving tabs and quitting keep; the hidden windows that hear Windows' broadcasts are made in one place; how a drag of files out of the window ended is decided once for every platform.
- The logs are split into the writer, the plain text and the index; tab dragging, the sidebar's drop logic and its width, and the appearance and terminal settings have files of their own.
- The frontend's stores are built one way, and the session registry's owners subscribe to it.
- Tests can connect with ZShell's own SSH client to a server in the test process, which covers forwarding rules and handing them over when a connection closes.

## [3.6.0] - 2026-10-17

### Bug Fixes

- In Settings › Known Hosts, the help icon next to the number of host keys lines up with the text.

### Performance

- The interface does less work while terminals are busy. A program that keeps changing its title, such as one showing a spinner, no longer makes the session list, the other tabs or the quick command bar redraw, and a running transfer no longer redraws the whole file list several times a second.

### Internal

- The interface is memoized by React Compiler, which compiles only components written by its rules: no refs read or written while rendering, the DOM measured in events and effects, and text that depends on the language taken from a hook. A test checks that every component and hook compiles.
- The components that shouldn't render again as the tabs change, and the rows of lists that render often (tabs, files, transfers, quick commands), get their callbacks as one object that stays the same; in development a warning says when that breaks.

## [3.5.1] - 2026-10-17

### Bug Fixes

- Terminals in background tabs no longer redraw their screen as their programs write output, which kept using CPU and memory while something ran in another tab (a build, a log being followed, a program with a spinner). They catch up when their tab is shown.
- Switching to a tab no longer shows its terminal blank for a moment before the text appears.

## [3.5.0] - 2026-10-15

The same app for users as 3.4.1: this version restructures the code, and saved sessions and settings are read and written as before.

### Internal

- The saved data (sessions, folders, proxies, forwarding rules, quick commands and settings) has a module of its own, which the vault, import and export and the features depend on rather than the other way round.
- Sessions get the saved passwords of their server, jump hosts and proxy with their route, instead of reading them from a global vault, and the stores read their part of the vault again by themselves once it is unlocked.
- The registry of live SSH connections, with their SFTP channels and forwarding rules, moves out of the SSH module, which now only makes the connection.
- Every command the frontend can call is in one place, and the names of the commands and of the events, with their payloads, are generated for the frontend and checked there.
- The frontend's code is split into logic, the calls to the backend and shared React hooks, and the stylesheet into parts by area, in the same order (the built stylesheet is unchanged).

## [3.4.1] - 2026-10-15

The same app as 3.4.0, which was not released because its Windows packages failed to build.

## [3.4.0] - 2026-10-15

### Features

- Host certificates: a server whose host key is certified by a certificate authority that `~/.ssh/known_hosts` trusts with an `@cert-authority` line (for example `@cert-authority *.example.com ssh-ed25519 AAAA…`) is trusted without confirming each host's key. As in OpenSSH, a certificate is asked for only from hosts with such a line. One that has expired, isn't valid yet or isn't for the host is reported in the terminal, and the key in it is checked as a plain host key instead; one whose key or authority is marked `@revoked`, or whose signature is invalid, is refused.
- Screen reader support in Settings › Terminal is now Automatic, On or Off. Automatic, the new default, turns it on while VoiceOver (macOS), Narrator, NVDA or JAWS (Windows) or Orca (Linux) is running, and off again when it stops, without restarting ZShell. If it was turned on before, it stays on, also after going back to an earlier version; if it was off, it becomes Automatic.

### Bug Fixes

- A server can no longer open channels toward ZShell that it wasn't asked for (a shell session, direct TCP/IP, X11, Unix sockets), each of which stayed open: they are refused, as OpenSSH does.
- During keyboard-interactive login, control characters in what the server shows are displayed as "�" (line breaks and tabs are kept), so a server can no longer clear the screen or fake a local prompt before you are logged in.
- On Windows, the host keys in `%ProgramData%\ssh\ssh_known_hosts` are trusted only when the file and its folder belong to the Administrators group or the system. Where only the OpenSSH client is installed, another user of the computer could create that folder with keys of their own and intercept connections unnoticed.
- Importing a sessions file refuses a session whose key file is on a network path (`\\server\share\…`), naming it. On Windows, merely editing or connecting such a session made Windows sign in to that server with your account.
- A key file is read only when it is a regular file of at most 1 MiB, so a key path pointing at a device or a pipe no longer fills memory or hangs. Reading and decrypting keys no longer holds up other tabs.
- With no editor chosen, Open in Editor no longer runs remote files that the system would run rather than open (`.cmd`, `.lnk` or `.url` on Windows, `.command` or `.terminal` on macOS): they open in the system's text editor. On Linux, ZShell asks for an editor to be chosen in Settings.
- Saving a file always creates its temporary copy as a new file, so a link planted in its place (when exporting sessions to a folder other users can write to, such as `/tmp`) can no longer make the save overwrite another of your files.
- Output that starts an OSC sequence and never ends it no longer fills memory while a text session log is written.
- A Telnet server that keeps sending option negotiation without reading the answers no longer makes them pile up in memory.
- A pane whose remote shell is killed by a signal (the OOM killer, an administrator's `kill`) stays open with its last output and says which signal it was, as for a local shell, instead of closing.
- The file panel's path field keeps what is being typed when the list refreshes by itself (after an upload, a save from the editor, or when following the terminal's folder).
- Ctrl+right-click and Ctrl+middle-click on a link in the terminal no longer open it in the browser.
- In a download with files that land on one local name (`README` and `readme` where case is ignored), a third file can no longer replace the renamed one.
- Resetting the vault while an unlock with the credential store was still waiting no longer leaves a vault that can't be opened on the next start.
- The export dialog lists session notes among what anyone can read in a file exported without the saved passwords.

### Performance

- A hidden terminal gives back its WebGL context at once, instead of keeping its memory in the web view until a garbage collection, and the glyph atlases of terminals drawing with WebGL start over once they pass about 64 MB: they keep every glyph in every color, so colorful Chinese text made them grow without end.
- On servers whose kernel doesn't list a process's children, the file panel finds the terminal's shell much faster over a slow connection, and doesn't look for it on servers with more than 4096 processes (the shell can still report its folder).

### Internal

- Reading and writing known_hosts no longer holds up other connections' work.
- The temporary folders of edited and dragged remote files are named only by ids of letters, digits and dashes.
- The tests of known_hosts and of the ssh_config import moved into files of their own.

## [3.3.0] - 2026-10-15

### Features

- Sessions and folders can be arranged in any order in the session list, mixed together: a session can go above a folder, and a folder below a session. Dragging a row onto the top or bottom edge of another places it before or after it, onto the middle of a folder puts it inside. The list looks as before until something is moved, exported sessions files keep the order, and imported sessions follow it.

### Bug Fixes

- The notes field in the session dialog looks like the other fields instead of a plain grey box.
- In Japanese and German, the fields of the session dialog no longer run past its right edge: a long encoding name on the Terminal page made every page too wide.
- The Windows executable's file properties and the macOS app's information show the copyright.
- The Windows executable's file properties and the Windows installers name the publisher as Boyin Chen instead of "boyin". If ZShell was installed in a folder of your own choosing with the Windows installer, this update may not find it and install into the default folder instead; uninstalling the old copy first avoids having two.

### Performance

- Typing a font name in Settings no longer saves the settings file on each key: the font still changes as it is typed, and is saved after a short pause or when the field is left.

## [3.2.0] - 2026-10-14

### Features

- The file panel opens in the folder the terminal's shell is in, has a button to go there at any time, and can follow the shell into each folder it goes to (a toggle in its toolbar, off by default). On Linux servers the folder is read from `/proc` without running anything on the server; elsewhere it is the folder the shell reports (OSC 7).
- While a path is typed in the file panel's path field, the matching folders are listed below it: Tab completes as a shell does, the arrow keys pick a folder, and Enter goes to it.
- Sessions have notes, on the General page of the session dialog. They show in the session's tooltip in the sidebar, the search finds sessions by them, and exports carry them.
- Opening a session from the sidebar while the current pane is that session, disconnected, connects again in that pane instead of opening another tab. The session menu's Connect in New Tab is now Connect.
- A tab in the background gets a ring on its status light when its terminal has new output, until it is shown. It can be turned off in Settings › Tabs.
- When a terminal rings its bell, a tab in the background shows a bell, a window in the background asks for attention (the Dock icon bounces, the taskbar button flashes), and the system's alert sound plays. A bell right after typing, such as a failed completion, doesn't count. Settings › Terminal can turn each of these off, and can also flash the terminal.
- A pane closes when its shell exits after the user typed into it (`exit`, Ctrl+D), as in Xshell. It stays open when the server ended the session before anything was typed, so what it said stays in sight.
- Links in the terminal open with ⌘-click on macOS and Ctrl+click on Windows and Linux, instead of on any click, and a tooltip says so. Links that programs mark (OSC 8, such as `ls --hyperlink`) open the same way and show their address. The context menu on a link has Open Link and Copy Link Address.
- Settings › Appearance has an App font field, for the font of the interface's own text.
- The settings open where they were when last closed, while the app runs.

### Bug Fixes

- A new session set to use a key file no longer starts with `~/.ssh/id_ed25519` when there is no such file, but with the first of OpenSSH's default keys found, and a key file missing from this computer is pointed out under the field.
- A quick double-click on a folder in the sidebar no longer opens the session that collapsing the folder moved under the pointer. Double-clicking a folder no longer opens all its sessions, and Open All Sessions asks first when there is more than one.
- Pressing the empty part of the session list clears the selection, so that new sessions and folders go at the top level again.
- The file panel's buttons no longer break their labels over two lines in Japanese or French: uploading files and uploading a folder are now in the menu of one Upload button.
- While files are dragged over the file panel, a drop zone over its toolbar takes them into the folder shown, which a folder holding only folders left nowhere to drop them. The list also scrolls while dragging near its top or bottom.
- On macOS in full screen, the app's name in place of the traffic lights opens the app menu.
- On macOS, the View menu's full screen item is named in the app's language.
- On Windows, Han characters in the interface are drawn in the forms of the interface language, not in Japanese forms on a Japanese Windows.

## [3.1.2] - 2026-10-14

### Bug Fixes

- Saving a session that another window deleted while it was being edited no longer brings it back under a new id, without its saved password and forwarding rules: the save fails with "Session profile not found".
- A host key that another connection (or `ssh`) added while an entry was being removed in Settings › Known Hosts is no longer lost; if known_hosts changed meanwhile, the removal says so instead.
- On macOS and Linux, new session logs can be read by your user account only, as ZShell's other files. Output left out of a log just before it stopped is noted at its end, and the last line of a log is no longer lost when it ends.
- Downloading a folder with two subfolders that the local disk takes for one name (`Dir` and `dir` where case is ignored, `a:b` and `a?b` on Windows) no longer merges them: the second gets a name of its own. Whether a disk ignores case is found out on the disk itself, so a case-sensitive macOS volume no longer renames files needlessly.
- Session and proxy names, remote file names and the replies of servers and HTTP proxies can no longer clear the screen, set the title or fake a prompt through ZShell's own messages in the terminal: their control characters are shown as "�". Output from the remote shell is not touched.
- On macOS and Linux, ZShell checks for an instance already running even when its configuration folder can't be made private, so that two can't overwrite each other's settings and sessions.
- On macOS and Linux, settings and sessions saved just before a power loss or system crash are no longer replaced by the previous version.
- A change to the settings or quick commands that can't be saved (a full disk, a read-only configuration folder) is undone, and a dialog says why, instead of seeming saved until the next start.
- A folder listing that arrives after the connection dropped no longer shows the file panel as connected or covers the disconnected notice.
- The terminal's context menu opens with the menu key or Shift+F10.
- Only the window that shows a session can act on it; moving a tab hands its sessions over to the window it goes to.
- Spell checking is off in every text field, such as a session's name, which underlined words and offered corrections on macOS.
- The focused pane's border is drawn on every side, also where it meets a divider.

### Internal

- The tests of the SSH sign-in and of the session logs moved into files of their own.

## [3.1.1] - 2026-10-13

### Bug Fixes

- On Windows, text fields in ZShell's dialogs, such as the session editor, no longer show a dropdown of values typed into them before.

## [3.1.0] - 2026-10-13

### Features

- ZShell is translated into Simplified Chinese, Japanese, German and French: the interface, the messages written to the terminal, error messages and the macOS menu bar. It follows the system's preferred languages, or the language chosen in Settings → Appearance, which applies at once to every window.
- Han characters in the interface and the terminal are drawn with Japanese fonts in Japanese and with Chinese fonts in Chinese; with another interface language, the first Chinese, Japanese or Korean language among the system's preferred ones decides.

## [3.0.2] - 2026-10-13

### Bug Fixes

- On Windows, a file dialog opened right after typing, such as the one `rz` opens, no longer hides the mouse pointer when the system's "Hide pointer while typing" setting is on.
- Dragging files out of the file browser of a ZShell window in the background no longer brings the window to the front, where a maximized window covered the Finder or Explorer window the files were meant for. On macOS the window stays where it is unless the press is released without dragging; on Windows it goes back behind that window as the drag starts.

## [3.0.1] - 2026-10-13

### Bug Fixes

- The saved SSH password is no longer filled in at a prompt that asks for another account's password, such as `Password for root:` while connecting as another user, where it only used up an attempt.
- On a Telnet server that leaves echoing to ZShell, a password typed at its prompt is no longer shown on the screen or written to the session's log.
- The file browser no longer lists entries whose names a server sent with a `/` or a NUL in them, and dragging out an item whose name can't be a local file name (`..`) fails at once, on macOS too, rather than leaving the name to Finder.
- The app lock now stays locked if it is asked to unlock before the sessions are unlocked, which ZShell's own screens never did.

## [3.0.0] - 2026-10-13

### Breaking Changes

- Sessions, folders, proxies, quick commands, saved passwords and the app lock's settings are saved in a new format, all in `vault.json`, and ZShell 3.0 does not read what earlier versions saved: the session list starts empty, and an encrypted `vault.json` of an earlier version shows as damaged, to be reset. Settings and logs are kept. Files exported by earlier versions cannot be imported either: recreate the sessions, or import them from `~/.ssh/config`.
- Exported sessions with their saved passwords are now encrypted as a whole, not only the passwords: the passphrase is asked for before the file's sessions are shown.

### Features

- At the first start, the sessions and passwords are encrypted with a key in the system's credential store (the macOS Keychain, the Windows Credential Manager, the Secret Service on Linux), or left unencrypted where there is none, instead of waiting for a choice; a dialog then says which, and offers a master password instead.
- A session or proxy is saved, copied and deleted with its password in one step, so a password can no longer be left behind or missing.

### Bug Fixes

- Settings › Security no longer offers the credential store on a Mac where the keychain can't be used.

## [2.13.0] - 2026-10-13

### Features

- Sessions and settings are saved in a folder named after the app: `~/Library/Application Support/ZShell` on macOS, `%APPDATA%\ZShell` on Windows and `~/.config/ZShell` on Linux, instead of `org.boyin.zshell`. Nothing is moved from the old folder; to keep using it, quit ZShell and rename it.

## [2.12.2] - 2026-10-13

### Bug Fixes

- Locking ZShell by hand no longer brings up Touch ID or Windows Hello at once: the lock screen asks when the window gets the focus again, or when Unlock is pressed.
- On Windows and Linux, the lock screen focuses its Unlock button when it opens, instead of the window's minimize button.
- The Check Now, Download and Skip buttons under Settings › About look like the other buttons in the settings.
- The hover background of a tab's close button is as tall as the other small buttons', instead of a thin strip.
- On Linux, the app icon is also installed as an SVG, so the desktop draws it sharp at any size.

## [2.12.1] - 2026-10-13

### Bug Fixes

- On Linux, a light or dark appearance chosen in the settings stays when the desktop's style changes, instead of the window frame and the page's own controls following the desktop; opening a new window no longer turns the others to the desktop's style for a moment.
- A window no longer shows the system's appearance for a moment as it opens, when another one is chosen in the settings.
- On Linux, the window buttons of a desktop that puts them on the left (`close,minimize,maximize:`) are mirrored at the right end of the tab bar, so the close button stays at the window's edge.
- On Linux, files dragged out of the file panel are never counted as delivered when the drag ends before they finish downloading.
- On Wayland, a tab released just as it leaves its window no longer starts a system drag after it has already been dropped.
- On Linux, when the running instance can't be reached, opening ZShell again says why on the terminal instead of exiting silently.

## [2.12.0] - 2026-10-13

### Features

- ZShell runs on Linux, made for GNOME on Wayland, the default desktop of Debian, Ubuntu and Fedora, with `.deb` and `.rpm` packages for x64:
  - The tab bar takes the place of the title bar, with the desktop's own window frame and its window buttons; double-clicking and right-clicking the tab bar do what they do on a title bar.
  - The appearance follows the desktop's light or dark style, and terminals default to the desktop's monospace font.
  - Remote files can be dragged out of the file panel into Files or onto the desktop, and tabs between windows.
  - The app lock asks for the account password (or a fingerprint, where set up) in the desktop's own dialog, and locks after the computer has been idle, when the screen locks, or before it sleeps.
  - Saved passwords and the vault's key go to the system keyring (GNOME Keyring, KWallet), and local terminals start the login shell.
  - Opening ZShell again brings its window forward.

### Internal

- The window and tab drag code is split into files by platform.

## [2.11.4] - 2026-10-12

### Bug Fixes

- A session dialog or the proxy settings already open show proxies added or removed in another window, instead of only once opened again.

### Internal

- The saved sessions, folders, proxies and recent sessions are kept in one store per window, which the sidebar, the tabs and the dialogs read directly instead of being handed them by the window. Reading them again ignores a reply older than the latest, which could leave a stale list after quick changes.
- The encodings offered and the defaults of a new session (terminal type, serial settings, quick command group) are generated from the backend's definitions instead of being written in the frontend as well.
- The session, saved session and vault code is split into files by part, with the tests in files of their own.

## [2.11.3] - 2026-10-12

### Bug Fixes

- When a download of several items fails, empty files and folders that had already arrived in full are kept; only the items from the one that failed onward are taken back.
- Choosing Replace for a file edited in another app, while another save of it is still uploading, is no longer lost, which brought the same question back.
- Uploading onto a folder whose contents can't be listed asks before replacing anything there.
- On Windows, items dragged out of the file panel whose names Windows can't tell apart (`README` and `readme`, or `a:b` and `a_b`) no longer land on the same file.
- Choosing to keep forwards when closing several tabs no longer hands over the forwards of a connection another tab still uses, once that tab is closed later with Stop chosen.
- Closing the last tab of a connection no longer waits for an SFTP session still being opened on it.
- A remote forward tries each address of its target in turn, so a target whose first address doesn't answer (IPv6 broken on this network) is still reached.
- A local or dynamic forward listening on a host name can be stopped or restarted while that name is looked up, and the lookup gives up after 15 seconds.
- SOCKS4 requests are accepted whatever their user id, which used to have to be UTF-8.
- A session log that falls behind no longer reports more missing output than there was.
- On Windows, a session log is never written to a device such as `NUL` or `COM1` when the file name template gives that name.
- ZMODEM receiving asks for the data again at once when the header starting it arrives damaged, as on a noisy serial line, instead of after 10 seconds; with more than 16 MiB left, the transfer used to fail.
- Unlocking says so when unencrypted copies of the sessions, left by a change of protection that did not finish, can't be deleted.
- Turning on encryption lists the session files that could not be read at an earlier start and were set aside in the clear, and offers to delete them.
- Output that arrives while a tab moves to another window is drawn at the size the program wrote it for, so progress bars and lines being edited no longer come out garbled.
- Moving a tab that is waiting to reconnect to another window connects once, from the new window.

## [2.11.2] - 2026-10-12

### Bug Fixes

- Closing a tab, pane or window, or quitting, asks first when files of its file panel are open in another app or a question waits there, even once disconnected: saves made afterwards, and those waiting for a reconnection, would no longer be uploaded.
- Uploading or downloading over an existing file no longer writes it in place when its temporary copy can't be created for another reason than permissions (a full disk, a quota), which left the original cut short. Files with very long names are now replaced through a temporary copy too.
- Change Permissions on a symbolic link starts from the permissions of what it points to, which are the ones that change, instead of the link's own `777`. Downloading over a local file keeps its temporary copy as private as the original.
- The saved password no longer answers one-time password prompts such as `One-time password (OATH)` or `Password + OTP`; only the account's password prompts get it.
- On macOS, the app lock no longer unlocks without authenticating when asking the system fails for a passing reason. Where there really is no way to authenticate left, the lock screen says so and unlocks only with Unlock Without Authenticating, rather than on its own.
- While the app lock is locked, the window title is ZShell rather than the session's name or the title the remote side set.
- Importing sessions shows which ones forward the SSH agent, type login commands or set environment variables, and the export dialog says that only saved passwords can be encrypted. Importing from the OpenSSH config shows `ForwardAgent yes` where a host has it.

### Internal

- The master password counts as authenticating for the app lock only when it actually opened the vault.

## [2.11.1] - 2026-10-12

### Bug Fixes

- A connection that drops while signing in no longer counts as a wrong password or a rejected key: automatic reconnection goes on, and the terminal no longer reports the saved password as rejected or asks for another on a connection that is gone. When the server itself ends the connection, as after too many failed attempts, ZShell shows the server's reason and stops, as OpenSSH does.
- SSH connections use TCP keepalives until signed in, so a connection that died while a password prompt waited fails as soon as the prompt is answered.
- A server that stops answering during the SSH handshake no longer leaves its connection (or proxy command) open after ZShell gives up or the tab is closed, one more with each automatic reconnection.

### Internal

- Removed a host key check on re-keying that never ran: the SSH library doesn't call it then.

## [2.11.0] - 2026-10-12

### Features

- An app menu on Windows: the ZShell icon and name at the top left of the title bar (or Ctrl+Shift+M) open File, Edit, View and Help menus, with Lock ZShell, Settings and Exit, so what the shortcuts do can also be found and chosen there. Items that don't apply in the window are disabled.
- A fuller macOS menu bar, with New Local Terminal, New Session, Search Sessions, importing and exporting sessions, Find, splitting, the file and port forwarding panels, the compose bar, quick commands, terminal full screen, Release Notes and Report an Issue.
- Update check: once a day, ZShell asks GitHub for the number of its latest version, and shows a newer one in Settings › About, where it can be downloaded from its page or skipped; a dot marks the way there. Check for Updates… checks at any time, and the daily check can be turned off in Settings › About. The Microsoft Store version is updated by the Store and doesn't check.
- On Windows, Ctrl+Shift+W with no tabs left closes the window, as ⌘W does on macOS.
- In terminal full screen, which hides the session list, the compose bar and the side panels, their keys do nothing.

### Bug Fixes

- ⌘Q quits from the unlock screen, where it did nothing.
- Lock ZShell with the app lock off shows the Security settings even when Settings is already open.

## [2.10.1] - 2026-10-12

### Bug Fixes

- With Settings open in two windows, a value typed but not yet saved in one is no longer replaced when the other window changes it; saving it then wins.
- The number of host keys, the known hosts list and the size of the session logs in Settings are read again when the window comes back to the front, so they include changes made in another window or outside ZShell.

## [2.10.0] - 2026-10-12

### Features

- Tabs can be dragged out of their window: dropped on another window's tab bar, the tab joins it where it was dropped (a marker shows where while dragging); dropped anywhere else, it opens in a new window under the pointer. Its connections keep running, as when it moves from the tab's menu. Escape cancels the drag.

## [2.9.0] - 2026-10-12

### Features

- Several windows: a tab's menu has Move to New Window and Move to “window”, and a saved session's menu has Connect in New Window. A moved tab keeps its connections, its split panes and what its terminals show, scrollback and full screen programs included. A window closes once its last tab has closed or moved away, unless it is the only one. A tab can't move while its file panel is transferring or editing files, or during a ZMODEM transfer; the file panel opens again in the new window, at the home folder.
- Settings, sessions, quick commands, the vault and the app lock stay the same in every window: what is changed or unlocked in one applies to all.
- Quitting asks once for all the windows, counting the busy tabs in each.

## [2.8.0] - 2026-10-12

### Features

- An app lock covers the window until you unlock it with Touch ID (or the login password) on macOS, or Windows Hello on Windows. Turn it on under Settings > Security. It locks when ZShell starts, after the computer has been idle for a time you choose, when the computer sleeps, locks its screen or switches user, and on demand with ⇧⌘L (Ctrl+Shift+L on Windows) or ZShell > Lock ZShell on macOS. Sessions keep running while it is locked. Its settings are kept with your sessions, so with encrypted sessions it can't be turned off by editing a file; entering the master password when ZShell starts unlocks it too.

### Bug Fixes

- macOS: opening ZShell while it is already running with the same data (another copy of the app, `open -n`, or the executable run directly) brings the running one forward, instead of starting a second instance that would overwrite its sessions and settings.

## [2.7.0] - 2026-10-12

### Features

- Sessions, folders, proxies, quick commands and saved passwords can be encrypted on your computer. Under Settings > Security, choose a master password (asked each time ZShell starts, and optionally remembered on this device), a key kept in the macOS Keychain or Windows Credential Manager (ZShell unlocks by itself), or no encryption (passwords then go in `passwords.json` beside the sessions). ZShell asks once which you want; until you choose, everything is kept as before. While the sessions are locked, the window shows only the unlock screen. A forgotten master password can't be recovered: the sessions can only be reset, so export them to keep a copy. Versions before 2.7 can't open encrypted sessions.
- Exported sessions can include their saved passwords, encrypted with a passphrase you give for the file; importing the file asks for it, or imports the sessions without their passwords. A file changed after it was exported (another host, a proxy command) doesn't give up the passwords.
- On macOS, the data folder and the files in it can be read only by your user account.

## [2.6.0] - 2026-10-11

### Features

- A status bar under the terminals shows the focused pane's connection (with the countdown to the next automatic reconnection), lights for data sent and received, the terminal type sent to the server (a serial port's line settings instead), the terminal's size and the cursor's position. The lights' tooltip has what the connection has sent and received and when data last came in. It can be turned off under Settings > Appearance.
- While a terminal is resized, its new size shows in its middle for a moment.
- Settings opens from a gear button in the sidebar header instead of the row at the bottom of the sidebar, so the session list runs to the bottom of the window. The sidebar is a little wider by default.

## [2.5.4] - 2026-10-11

### Bug Fixes

- Serial on macOS: closing a tab while a paste or ZMODEM upload waits for a device holding the line with flow control lets go of the device at once, instead of keeping it busy (reconnecting failed with the device in use) for as long as the device held the line.
- Uploading over an existing file no longer removes the original before the new file is in place: it is moved aside first and removed only after. If the connection is lost in between, both are kept and the error says where.
- A session log continued after a reconnection no longer mixes the line marking the reconnection into the previous connection's last output when the log's folder is slow (a network drive), and reconnecting no longer waits for that folder.

### Internal

- A local terminal stops reading its output once its tab is closed, which closes the terminal for programs still using it, instead of waiting for them to let go. macOS does this itself when the shell exits; this prepares for Linux, which doesn't.

## [2.5.3] - 2026-10-11

### Bug Fixes

- A session's login commands stop once you start typing, pasting or sending from the compose bar, instead of only on Ctrl+C. Before, a short pause while typing let the next command be added to your unfinished line (`ls` became `lscd /srv`).
- Duplicating or splitting a quick connect's SSH tab opens the new shell on the connection already open, as for saved sessions, instead of connecting and asking for the password again.
- Reconnecting a tab by hand waits for the old connection to close, so the forwarding rules running on it can't be left on it and stop with it.
- File panel: when the tab reconnects while its first folder is still opening, the late answer no longer leaves the panel empty or shows an error over the new listing. Uploads waiting on the question about replacing files, "Download to…" and deletes use the connection there is when you answer, instead of failing on the closed one or running on the old one.
- ZMODEM: Ctrl+C cancels an upload to `lrz` over a slower link at once, instead of only after the file; choosing files at the moment the transfer is cancelled no longer starts it.
- Two remote (`-R`) forwarding rules on the same port with different addresses: a connection the server reports with an address spelled differently is refused instead of going to whichever rule came first, possibly the wrong local service.
- Removing a host key keeps `known_hosts.old` as private as `known_hosts` instead of readable by everyone, and a `~/.ssh` folder created to record a host key is readable only by you, as OpenSSH makes it.
- A known_hosts line with many `*` in its host pattern no longer holds up connecting while host keys are checked.
- Importing `ProxyCommand ssh -W %h:%p user@2001:db8::1` from an OpenSSH config keeps the IPv6 jump host whole instead of taking its last group for a port.

## [2.5.2] - 2026-10-10

### Bug Fixes

- A session's host can be an IPv6 address written in brackets, such as `[2001:db8::1]`: it is saved without them instead of being refused. The port still goes in its own field.

## [2.5.1] - 2026-10-10

The same app as 2.5.0, which was not released because its Windows packages failed to build.

## [2.5.0] - 2026-10-10

### Features

- Full screen for the terminals: ⌘↩ on macOS or F11 on Windows (also Full Screen in the terminal's menu) shows only the current tab's terminals, split panes included, without the session list, the tabs or the bars. The same keys bring everything back.

### Bug Fixes

- macOS: after the window left full screen, typing reached the terminal only once it was clicked.

### Internal

- Every keyboard shortcut is declared in one table, from which the keys, the labels in menus and the list in the settings come, to prepare for custom shortcuts and Linux. The macOS list of shortcuts now writes Ctrl+Tab as ⌃⇥, as macOS menus do.

## [2.4.3] - 2026-10-10

### Features

- The session list shows an icon for each session's protocol (a terminal for SSH, an Ethernet jack for Telnet, a serial connector for serial) in the column of the folders' triangles, instead of starting with an empty gap; sessions in a folder now start below its name.

### Bug Fixes

- Microsoft Store version: the app's icon in the Start menu and the taskbar is sharp at every display scale.

## [2.4.2] - 2026-10-10

### Bug Fixes

- Connecting to a host whose first address doesn't answer (IPv6 broken on this network, a dead server among several) now tries its next address after 15 seconds, as OpenSSH does, instead of failing.
- A session set to use a key file asks for the key's passphrase only once the server accepts the key, so a key the server doesn't know no longer has you type the passphrase for nothing.
- Proxy commands such as cloudflared are found again after the login shell was too slow to answer right after the computer started, without restarting ZShell.
- Two tabs of a session connecting at the same moment (after the computer wakes) no longer both start the same forwarding rule, one of them failing with its port in use.
- A dynamic (`-D`) forwarding rule no longer shows an error when a browser drops a connection it opened ahead of time.
- Reconnecting a tab by hand twice in a row (the first new connection still at its password prompt) keeps the forwarding rules that were running.
- ZMODEM: a file whose end couldn't be written (a full disk) is removed instead of being left truncated; an upload over a noisy serial line survives a garbled reply from the receiver; a download no longer fails when much data is still in flight after an error; and after an upload is cancelled, the receiver's last replies no longer show as text or ask about a new transfer.
- Dragging files out of the file panel on macOS: dropping on the Dock or a floating panel in front of the window is no longer taken as a drop on the window.
- Downloading a folder that holds two names the local disk takes for the same (`README` and `readme`) keeps both files, the second with a number added.
- A file opened in an editor from the file panel that changed on the server during its download is noticed: saving asks before overwriting the change.
- A local shell that exits while the terminal is still catching up with its output (`cat big.log; exit`) shows all of it before saying the process exited.
- Cleaning up old session logs on a network drive that is asleep or gone no longer holds up open sessions.
- Typing in a Telnet session that echoes locally no longer garbles a character when the server's output stopped in the middle of a GBK character.
- Clicks on the right of a wide terminal (past column 95) reach programs that use the default mouse encoding.
- The OpenSSH import dialog shows the hosts of the file chosen last, even if the default file took longer to read.
- Importing from an OpenSSH config: options after an `Include` inside a `Host` block stay with that host, and a jump host already saved as an SSH session is used even when a Telnet session has the same name.
- Importing sessions from a ZShell export no longer uses a proxy here of another kind just because it has the same name; the file's proxy is imported instead, with a number added to its name.
- Deleting a folder puts its subfolders where it was, also those listed before it.
- On Windows, closing a local shell no longer asks about a program running in it when the program belongs to an earlier process with the same id, and a file dragged out while the app quits no longer keeps a CPU core busy.

## [2.4.1] - 2026-10-10

### Bug Fixes

- Opening the file panel on a server without SFTP (its `Subsystem sftp` turned off) says so at once instead of waiting two minutes, during which every other file operation on the tab waited too.
- On Windows, a serial line held back by RTS/CTS or XON/XOFF flow control for more than 10 ms (pasting, uploading with ZMODEM) no longer ends the session as disconnected.
- A session that finished opening just after the window reloaded (it waits for its log to open) no longer keeps running unseen: a local shell no longer stays behind, and an SSH connection no longer keeps its automatic forwarding rules' ports, which the reloaded window's connection then skipped.
- Deleting a session or proxy succeeds even when the keychain refuses to remove its password (macOS asks again for each item after an update), and says that the password is still in the keychain, instead of reporting an error while the sidebar kept showing it and saving from the still-open dialog brought it back. Duplicating a session shows the copy, saying so if its password couldn't be copied, instead of failing so that trying again made another copy.
- A terminal that becomes active while a dialog is open (the focused pane exited while the settings were open) no longer takes the keyboard focus from the dialog, where Escape and Tab went to the shell behind it; it takes the focus once the dialog closes.

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
