#!/usr/bin/env python3
"""A SOCKS5 or HTTP CONNECT proxy for end-to-end tests of ZShell (see docs/DEVELOPMENT.md).
Logs each request (the destination as the client sent it, and the user name) to stdout.
With --user and --password, clients must authenticate (RFC 1929 for SOCKS5, Basic for HTTP).

usage: proxy.py socks5|http PORT [--user NAME --password SECRET]
"""
import base64, socket, struct, sys, threading

kind, port = sys.argv[1], int(sys.argv[2])
user = sys.argv[sys.argv.index("--user") + 1] if "--user" in sys.argv else None
password = sys.argv[sys.argv.index("--password") + 1] if "--password" in sys.argv else None


def log(*args):
    print(*args, flush=True)


def recv_exact(sock, n):
    data = b""
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            raise ConnectionError("closed")
        data += chunk
    return data


def pipe(src, dst):
    try:
        while data := src.recv(65536):
            dst.sendall(data)
    except OSError:
        pass
    for sock in (src, dst):
        try:
            sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass


def relay(client, host, port):
    try:
        target = socket.create_connection((host, port), timeout=10)
    except OSError as e:
        log(f"  connect failed: {e}")
        return None
    target.settimeout(None)
    return target


def socks5(client):
    version, count = recv_exact(client, 2)
    methods = recv_exact(client, count)
    method = 2 if user else 0
    if method not in methods:
        client.sendall(b"\x05\xff")
        log(f"no acceptable method in {list(methods)}")
        return
    client.sendall(bytes([5, method]))
    name = None
    if user:
        _, length = recv_exact(client, 2)
        name = recv_exact(client, length).decode()
        secret = recv_exact(client, recv_exact(client, 1)[0]).decode()
        ok = name == user and secret == password
        client.sendall(b"\x01" + (b"\x00" if ok else b"\x01"))
        if not ok:
            log(f"rejected user {name!r}")
            return
    _, command, _, address_type = recv_exact(client, 4)
    if address_type == 1:
        host = socket.inet_ntoa(recv_exact(client, 4))
    elif address_type == 3:
        host = recv_exact(client, recv_exact(client, 1)[0]).decode()
    else:
        host = socket.inet_ntop(socket.AF_INET6, recv_exact(client, 16))
    (dport,) = struct.unpack(">H", recv_exact(client, 2))
    log(f"SOCKS5 CONNECT {host}:{dport} (address type {address_type}, user {name})")
    target = relay(client, host, dport)
    client.sendall(bytes([5, 0 if target else 5, 0, 1, 0, 0, 0, 0, 0, 0]))
    return target


def http(client):
    head = b""
    while not head.endswith(b"\r\n\r\n"):
        head += recv_exact(client, 1)
    lines = head.decode().split("\r\n")
    log(lines[0])
    method, authority, _ = lines[0].split(" ", 2)
    if method != "CONNECT":
        client.sendall(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")
        return
    if user:
        expected = "Basic " + base64.b64encode(f"{user}:{password}".encode()).decode()
        sent = [line.split(":", 1)[1].strip() for line in lines if line.lower().startswith("proxy-authorization:")]
        if sent != [expected]:
            log(f"  rejected credentials {sent}")
            client.sendall(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"test\"\r\n\r\n")
            return
    host, _, dport = authority.rpartition(":")
    target = relay(client, host.strip("[]"), int(dport))
    client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n" if target else b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
    return target


def serve(client):
    try:
        target = socks5(client) if kind == "socks5" else http(client)
    except (ConnectionError, ValueError) as e:
        log(f"  error: {e}")
        target = None
    if not target:
        client.close()
        return
    threading.Thread(target=pipe, args=(target, client), daemon=True).start()
    pipe(client, target)


server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", port))
server.listen()
log(f"{kind} proxy on 127.0.0.1:{port}" + (f", user {user}" if user else ""))
while True:
    sock, _ = server.accept()
    threading.Thread(target=serve, args=(sock,), daemon=True).start()
