#!/usr/bin/env python3
"""A fake serial device for end-to-end tests of ZShell (see docs/DEVELOPMENT.md): a pseudo
terminal whose other end runs a shell, like a board's serial console. The device path is a
symlink, kept across restarts, so "unplugging" (stopping the script) and "plugging in"
again (restarting it) can be tested.

usage: fakeserial.py LINK [--home DIR]
"""
import os, pty, select, signal, sys, tty

link = sys.argv[1]
home = sys.argv[sys.argv.index("--home") + 1] if "--home" in sys.argv else os.getcwd()

master, slave = os.openpty()
name = os.ttyname(slave)
tty.setraw(slave)
os.close(slave)
if os.path.lexists(link):
    os.remove(link)
os.symlink(name, link)
print("device", link, "->", name, flush=True)

pid, shell = pty.fork()
if pid == 0:
    os.chdir(home)
    env = {"TERM": "vt100", "HOME": home, "PATH": "/opt/homebrew/bin:/usr/bin:/bin", "LANG": "en_US.UTF-8",
           "PS1": "board # "}
    os.execve("/bin/sh", ["/bin/sh", "-i"], env)


def stop(*_):
    os.kill(pid, 9)
    sys.exit(0)


signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
# Keep a slave open ourselves until ZShell opens it, so the master sees no hang-up before.
keep = os.open(name, os.O_RDWR | os.O_NOCTTY)
# Reopening resets the line settings (echo on would loop the shell's output back to it).
tty.setraw(keep)
while True:
    r, _, x = select.select([master, shell], [], [master])
    if master in r:
        try:
            data = os.read(master, 65536)
        except OSError:
            continue
        if data:
            os.write(shell, data)
    if shell in r:
        try:
            data = os.read(shell, 65536)
        except OSError:
            stop()
        if not data:
            stop()
        os.write(master, data)
