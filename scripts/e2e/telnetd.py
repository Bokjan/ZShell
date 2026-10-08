#!/usr/bin/env python3
"""A minimal Telnet server for end-to-end tests of ZShell (see docs/DEVELOPMENT.md): option
negotiation, a login prompt (alice / secret), then a shell in a pty. Logs the negotiation
and special commands to stdout. A break makes the shell print BREAK-RECEIVED.

usage: telnetd.py PORT [--binary] [--no-echo] [--home DIR]
"""
import fcntl, os, pty, select, socket, struct, sys, termios, threading

IAC, DONT, DO, WONT, WILL, SB, SE, BRK, NOP = 255, 254, 253, 252, 251, 250, 240, 243, 241
BINARY, ECHO, SGA, TTYPE, NAWS = 0, 1, 3, 24, 31
NAMES = {BINARY: "BINARY", ECHO: "ECHO", SGA: "SGA", TTYPE: "TTYPE", NAWS: "NAWS"}
VERBS = {DO: "DO", DONT: "DONT", WILL: "WILL", WONT: "WONT"}

port = int(sys.argv[1])
binary = "--binary" in sys.argv
echo = "--no-echo" not in sys.argv
home = sys.argv[sys.argv.index("--home") + 1] if "--home" in sys.argv else os.getcwd()


def log(*args):
    print(*args, flush=True)


class Conn:
    def __init__(self, sock, addr):
        self.sock, self.addr = sock, addr
        self.state, self.verb, self.sb = "data", None, bytearray()
        self.size, self.ttype = (80, 24), "unknown"
        self.cr = False
        self.shell_fd = None

    def send(self, data):
        self.sock.sendall(data)

    def parse(self, data):
        """Returns plain data; handles commands."""
        out = bytearray()
        for b in data:
            if self.state == "data":
                if b == IAC:
                    self.state = "iac"
                elif self.cr and b == 0:
                    self.cr = False  # CR NUL -> CR
                else:
                    out.append(b)
                    self.cr = b == 13
            elif self.state == "iac":
                self.state = "data"
                if b == IAC:
                    out.append(IAC)
                elif b in VERBS:
                    self.verb, self.state = b, "opt"
                elif b == SB:
                    self.sb, self.state = bytearray(), "sb"
                elif b == BRK:
                    log("<- BRK")
                    if self.shell_fd is not None:
                        os.write(self.shell_fd, b"echo BREAK-RECEIVED\r")
                else:
                    log("<- command", b)
            elif self.state == "opt":
                self.state = "data"
                log("<-", VERBS[self.verb], NAMES.get(b, b))
                if self.verb == DO and b == BINARY:
                    self.send(bytes([IAC, WILL if binary else WONT, BINARY]))
                elif self.verb == WILL and b == BINARY:
                    self.send(bytes([IAC, DO if binary else DONT, BINARY]))
                elif self.verb == WILL and b == TTYPE:
                    self.send(bytes([IAC, SB, TTYPE, 1, IAC, SE]))
                elif self.verb == DO and b in (ECHO, SGA) or self.verb == WILL and b in (NAWS, SGA):
                    pass  # answers to our offers
                elif self.verb in (DO, WILL):
                    self.send(bytes([IAC, WONT if self.verb == DO else DONT, b]))
            elif self.state == "sb":
                if b == IAC:
                    self.state = "sbiac"
                else:
                    self.sb.append(b)
            elif self.state == "sbiac":
                if b == SE:
                    self.state = "data"
                    self.subneg()
                else:
                    self.sb.append(b)
                    self.state = "sb"
        return bytes(out)

    def subneg(self):
        sb = bytes(self.sb)
        if sb[:1] == bytes([NAWS]) and len(sb) >= 5:
            self.size = struct.unpack(">HH", sb[1:5])
            log("<- NAWS", self.size)
            if self.shell_fd is not None:
                fcntl.ioctl(self.shell_fd, termios.TIOCSWINSZ, struct.pack("HHHH", self.size[1], self.size[0], 0, 0))
        elif sb[:2] == bytes([TTYPE, 0]):
            self.ttype = sb[2:].decode()
            log("<- TTYPE IS", self.ttype)
        else:
            log("<- SB", list(sb))

    def read_line(self, hidden):
        line = bytearray()
        while True:
            data = self.sock.recv(1024)
            if not data:
                raise EOFError
            for b in self.parse(data):
                if b in (13, 10):
                    if b == 13 or line:
                        self.send(b"\r\n")
                        return line.decode(errors="replace")
                elif b in (8, 127):
                    if line:
                        line.pop()
                        if echo and not hidden:
                            self.send(b"\b \b")
                elif b >= 32:
                    line.append(b)
                    if echo and not hidden:
                        self.send(bytes([b]))

    def run(self):
        log("connection from", self.addr)
        offers = [IAC, DO, TTYPE, IAC, DO, NAWS, IAC, WILL, SGA]
        if echo:
            offers += [IAC, WILL, ECHO]
        self.send(bytes(offers))
        try:
            for _ in range(3):
                self.send(b"\r\nZShell test telnetd\r\nlogin: ")
                user = self.read_line(False)
                self.send(b"Password: ")
                password = self.read_line(True)
                log("login attempt", repr(user), repr(password))
                if (user, password) == ("alice", "secret"):
                    break
                self.send(b"\r\nLogin incorrect\r\n")
            else:
                return
            self.shell()
        except (EOFError, OSError):
            pass
        finally:
            log("connection closed", self.addr)
            self.sock.close()

    def shell(self):
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(home)
            env = {"TERM": self.ttype, "HOME": home, "PATH": "/opt/homebrew/bin:/usr/bin:/bin", "LANG": "en_US.UTF-8",
                   "PS1": "alice@telnetd $ "}
            os.execve("/bin/sh", ["/bin/sh", "-i"], env)
        self.shell_fd = fd
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", self.size[1], self.size[0], 0, 0))
        try:
            while True:
                r, _, _ = select.select([self.sock, fd], [], [])
                if self.sock in r:
                    data = self.sock.recv(65536)
                    if not data:
                        break
                    plain = self.parse(data)
                    if plain:
                        os.write(fd, plain)
                if fd in r:
                    try:
                        data = os.read(fd, 65536)
                    except OSError:
                        break
                    if not data:
                        break
                    self.send(data.replace(b"\xff", b"\xff\xff"))
        finally:
            os.close(fd)
            try:
                os.kill(pid, 9)
                os.waitpid(pid, 0)
            except OSError:
                pass


server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", port))
server.listen()
log("listening on", port, "binary" if binary else "nvt", "echo" if echo else "no-echo")
while True:
    sock, addr = server.accept()
    threading.Thread(target=Conn(sock, addr).run, daemon=True).start()
