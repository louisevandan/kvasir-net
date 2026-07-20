#!/usr/bin/env python3
"""Dial-out bridge between a local expert-dispatch TCP endpoint and a hub's
/api/expert-relay WebSocket (M2 of docs/design/moe-expert-sharding.md).

The expert-dispatch data plane is one long-lived raw TCP stream per
(backbone, worker) pair. When the two sides can't dial each other directly
(NAT phone worker, remote backbone), each side that must dial OUT runs this
bridge; the hub's /api/expert-relay bridges the WS legs to the registered
TCP listener.

  worker mode   — the worker host dials out: connect the hub WS AND the local
                  linkcpp-expert-worker --serve port, then pipe.
                    expert-relay-dial.py --mode worker \
                      --hub ws://HUB:19000/api/expert-relay --session S \
                      --local 127.0.0.1:52800 [--token T]
  backbone mode — the backbone host dials out: listen on a local port; when
                  the backbone (linkcpp-moe-verify/linkcpp-server
                  --dispatch-port) connects, dial the hub WS and pipe.
                    expert-relay-dial.py --mode backbone \
                      --hub ws://HUB:19001/api/expert-relay --session S \
                      --local 127.0.0.1:52901 [--token T]

Stdlib only (no websockets/aiohttp dependency) so it runs on any node host:
a minimal RFC 6455 client — masked binary frames out, server frames in,
ping answered with pong. ws:// and wss:// both supported.
"""

import argparse
import base64
import os
import socket
import ssl
import struct
import sys
import threading
import urllib.parse


def log(msg):
    print("[expert-relay-dial] " + msg, file=sys.stderr, flush=True)


# ---- minimal RFC 6455 client ------------------------------------------------

def ws_connect(url, token=""):
    u = urllib.parse.urlsplit(url)
    tls = u.scheme == "wss"
    host = u.hostname
    port = u.port or (443 if tls else 80)
    q = urllib.parse.parse_qsl(u.query)
    if token:
        q.append(("token", token))
    path = u.path + ("?" + urllib.parse.urlencode(q) if q else "")

    sock = socket.create_connection((host, port), timeout=15)
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    if tls:
        sock = ssl.create_default_context().wrap_socket(sock, server_hostname=host)

    key = base64.b64encode(os.urandom(16)).decode()
    req = ("GET {} HTTP/1.1\r\nHost: {}:{}\r\nUpgrade: websocket\r\n"
           "Connection: Upgrade\r\nSec-WebSocket-Key: {}\r\n"
           "Sec-WebSocket-Version: 13\r\n\r\n").format(path, host, port, key)
    sock.sendall(req.encode())

    resp = b""
    while b"\r\n\r\n" not in resp:
        chunk = sock.recv(4096)
        if not chunk:
            raise ConnectionError("ws handshake: connection closed")
        resp += chunk
    status = resp.split(b"\r\n", 1)[0].decode(errors="replace")
    if " 101 " not in status + " ":
        raise ConnectionError("ws handshake refused: " + status)
    sock.settimeout(None)
    return sock


def _recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("ws: connection closed")
        buf += chunk
    return buf


def ws_send_binary(sock, payload, lock):
    n = len(payload)
    if n < 126:
        head = struct.pack("!BB", 0x82, 0x80 | n)
    elif n < 65536:
        head = struct.pack("!BBH", 0x82, 0x80 | 126, n)
    else:
        head = struct.pack("!BBQ", 0x82, 0x80 | 127, n)
    # RFC 6455 requires client frames to be masked but allows any key; a zero
    # key makes the mask a no-op, so combine-sized frames skip a pure-Python
    # per-byte XOR on the dispatch hot path.
    with lock:
        sock.sendall(head + b"\x00\x00\x00\x00" + payload)


def ws_recv(sock, lock):
    """Returns the next binary/text payload, transparently answering pings.
    Returns None on a clean close frame."""
    while True:
        b0, b1 = _recv_exact(sock, 2)
        opcode = b0 & 0x0F
        n = b1 & 0x7F
        if n == 126:
            (n,) = struct.unpack("!H", _recv_exact(sock, 2))
        elif n == 127:
            (n,) = struct.unpack("!Q", _recv_exact(sock, 8))
        mask = _recv_exact(sock, 4) if b1 & 0x80 else b""
        payload = _recv_exact(sock, n) if n else b""
        if mask:
            payload = bytes(c ^ mask[i & 3] for i, c in enumerate(payload))
        if opcode == 0x8:      # close
            return None
        if opcode == 0x9:      # ping -> pong
            m = os.urandom(4)
            with lock:
                sock.sendall(struct.pack("!BB", 0x8A, 0x80 | len(payload)) + m
                             + bytes(c ^ m[i & 3] for i, c in enumerate(payload)))
            continue
        if opcode == 0xA:      # pong
            continue
        return payload


# ---- piping ------------------------------------------------------------------

def bridge(tcp_sock, ws_sock):
    """Pipe until either side ends; then shut both down."""
    send_lock = threading.Lock()
    done = threading.Event()

    def tcp_to_ws():
        try:
            while not done.is_set():
                data = tcp_sock.recv(65536)
                if not data:
                    break
                ws_send_binary(ws_sock, data, send_lock)
        except OSError:
            pass
        finally:
            done.set()

    def ws_to_tcp():
        try:
            while not done.is_set():
                data = ws_recv(ws_sock, send_lock)
                if data is None:
                    break
                tcp_sock.sendall(data)
        except (OSError, ConnectionError):
            pass
        finally:
            done.set()

    threads = [threading.Thread(target=tcp_to_ws, daemon=True),
               threading.Thread(target=ws_to_tcp, daemon=True)]
    for t in threads:
        t.start()
    done.wait()
    for s in (tcp_sock, ws_sock):
        try:
            s.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        try:
            s.close()
        except OSError:
            pass
    for t in threads:
        t.join(timeout=5)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--mode", choices=["worker", "backbone"], required=True)
    ap.add_argument("--hub", required=True, help="ws(s)://host:port/api/expert-relay")
    ap.add_argument("--session", required=True)
    ap.add_argument("--token", default=os.environ.get("LINKCPP_NODE_TOKEN", ""))
    ap.add_argument("--local", required=True, help="host:port — worker: the local "
                    "--serve port to connect; backbone: the local port to listen on")
    ap.add_argument("--once", action="store_true", help="worker mode: exit after one bridge")
    ap.add_argument("--retry-sec", type=float, default=5.0, help="worker mode: re-dial interval")
    args = ap.parse_args()

    host, port = args.local.rsplit(":", 1)
    port = int(port)
    hub = args.hub + ("&" if "?" in args.hub else "?") + "session=" + urllib.parse.quote(args.session)

    if args.mode == "worker":
        # The hub bridges this WS to the backbone's dispatch listener, which only
        # opens once the backbone has its model loaded — so keep dialing until a
        # bridge sticks, and re-dial when a stream ends (backbone restart, churn).
        import time
        while True:
            try:
                ws = ws_connect(hub, args.token)
                log("hub WS connected: " + args.hub)
                tcp = socket.create_connection((host, port))
                tcp.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                log("local worker connected: %s:%d — bridging" % (host, port))
                bridge(tcp, ws)
                log("bridge ended")
            except (OSError, ConnectionError) as exc:
                log("dial failed: %r" % (exc,))
            if args.once:
                return
            time.sleep(args.retry_sec)

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((host, port))
    srv.listen(1)
    log("backbone mode: listening on %s:%d, one bridge per connection" % (host, port))
    while True:
        tcp, peer = srv.accept()
        tcp.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        log("backbone connected from %s:%d — dialing hub" % peer)
        try:
            ws = ws_connect(hub, args.token)
        except (OSError, ConnectionError) as exc:
            log("hub dial failed: %r" % (exc,))
            tcp.close()
            continue
        bridge(tcp, ws)
        log("bridge ended, waiting for next connection")


if __name__ == "__main__":
    main()
