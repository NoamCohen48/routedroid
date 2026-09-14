#!/usr/bin/env python3
"""Fake Android client for the Phase 0 namespace lab (stdlib only).

Models the Android side of protocol/phase0-draft.md:

  * creates an IFF_TUN|IFF_NO_PI interface (the "VpnService" tun) inside the
    phone network namespace,
  * connects to the host's loopback port (inside the host namespace -- the
    process switches namespaces with setns, exactly like `adb reverse` bridges
    the phone's 127.0.0.1:DEVICE_PORT to the host's 127.0.0.1:HOST_PORT),
  * HELLO -> HELLO_ACK -> CONFIGURE_VPN -> (configure tun) -> VPN_READY,
  * pumps one whole IPv4 packet per IP_PACKET frame in both directions with
    IPv4 validation before injection, answers PING, exits on STOP/ERROR.

Must run as root (TUN creation, setns, ip commands).
"""

import argparse
import ctypes
import fcntl
import json
import os
import select
import signal
import socket
import struct
import subprocess
import sys

HEADER = struct.Struct("!IBBH")
HELLO, HELLO_ACK, CONFIGURE_VPN, VPN_READY, VPN_ERROR = 0x01, 0x02, 0x03, 0x04, 0x05
IP_PACKET, PING, PONG, STOP, ERROR = 0x10, 0x20, 0x21, 0x30, 0x7F
MAX_CONTROL = 65536
EMPTY_TYPES = {PING, PONG, STOP}

TUNSETIFF = 0x400454CA
IFF_TUN = 0x0001
IFF_NO_PI = 0x1000
CLONE_NEWNET = 0x40000000


def log(msg):
    print(f"[fake-android] {msg}", file=sys.stderr, flush=True)


# ---------------------------------------------------------------- namespaces
def setns(path):
    fd = os.open(path, os.O_RDONLY)
    try:
        if hasattr(os, "setns"):
            os.setns(fd, CLONE_NEWNET)
        else:  # Python < 3.12
            libc = ctypes.CDLL(None, use_errno=True)
            if libc.setns(fd, CLONE_NEWNET) != 0:
                e = ctypes.get_errno()
                raise OSError(e, os.strerror(e))
    finally:
        os.close(fd)


# ---------------------------------------------------------------------- TUN
def open_tun(name):
    fd = os.open("/dev/net/tun", os.O_RDWR | os.O_CLOEXEC)
    ifr = struct.pack("16sH", name.encode(), IFF_TUN | IFF_NO_PI) + b"\0" * 22
    fcntl.ioctl(fd, TUNSETIFF, ifr)
    return fd


def ipv4_ok(pkt):
    if len(pkt) < 20:
        return False
    ver, ihl = pkt[0] >> 4, pkt[0] & 0x0F
    if ver != 4 or ihl < 5:
        return False
    total = struct.unpack("!H", pkt[2:4])[0]
    return total == len(pkt) and total >= ihl * 4


# ------------------------------------------------------------------- framing
class ProtocolError(Exception):
    pass


def encode(mtype, body=b""):
    return HEADER.pack(len(body), 0, mtype, 0) + body


def validate_header(length, version, mtype, flags, mtu):
    if version != 0:
        raise ProtocolError(f"version {version}")
    if flags != 0:
        raise ProtocolError(f"flags 0x{flags:04x}")
    if mtype == IP_PACKET:
        if not 20 < length <= mtu:
            raise ProtocolError(f"IP_PACKET length {length}")
    elif mtype in EMPTY_TYPES:
        if length != 0:
            raise ProtocolError(f"type 0x{mtype:02x} with body")
    elif mtype in (HELLO, HELLO_ACK, CONFIGURE_VPN, VPN_READY, VPN_ERROR, ERROR):
        if length == 0 or length > MAX_CONTROL:
            raise ProtocolError(f"control length {length}")
    else:
        raise ProtocolError(f"unknown type 0x{mtype:02x}")


class FrameReader:
    """Incremental decoder; only allocates a body after the header is valid."""

    def __init__(self, mtu):
        self.mtu = mtu
        self.buf = bytearray()

    def feed(self, data):
        self.buf += data

    def frames(self):
        while True:
            if len(self.buf) < HEADER.size:
                return
            length, version, mtype, flags = HEADER.unpack_from(self.buf)
            validate_header(length, version, mtype, flags, self.mtu)
            end = HEADER.size + length
            if len(self.buf) < end:
                return
            body = bytes(self.buf[HEADER.size:end])
            del self.buf[:end]
            yield mtype, body


# ------------------------------------------------------------------ session
class FakeAndroid:
    def __init__(self, args):
        self.args = args
        self.mtu = 1400
        self.state = "connected"
        self.stats = {"tun_to_host": 0, "host_to_tun": 0, "drop_invalid": 0}

    def ip_in_phone_ns(self, *cmd):
        full = ["ip", "-n", self.args.tun_netns] if self.args.tun_netns else ["ip"]
        subprocess.run(full + list(cmd), check=True)

    def configure(self, cfg):
        """What VpnService.Builder.establish() would do, using iproute2."""
        name = self.args.tun_name
        self.mtu = cfg["mtu"]
        self.ip_in_phone_ns("link", "set", "dev", name, "mtu", str(self.mtu), "up")
        for a in cfg["addresses"]:
            self.ip_in_phone_ns("addr", "add", f"{a['address']}/{a['prefix']}", "dev", name)
        for r in cfg["routes"]:
            self.ip_in_phone_ns("route", "add", f"{r['address']}/{r['prefix']}", "dev", name)
        # Android would also install cfg["dns"]; nothing to do in a namespace.
        return {
            "addresses": [f"{a['address']}/{a['prefix']}" for a in cfg["addresses"]],
            "mtu": self.mtu,
        }

    def run(self):
        a = self.args
        # 1. TUN inside the phone namespace (VpnService.establish()).
        if a.tun_netns:
            setns(f"/run/netns/{a.tun_netns}")
        tun = open_tun(a.tun_name)
        log(f"created tun {a.tun_name} in netns {a.tun_netns or 'current'}")
        # 2. Socket inside the host namespace (adb reverse bridging loopback).
        if a.connect_netns:
            setns(f"/run/netns/{a.connect_netns}")
        sock = socket.create_connection(("127.0.0.1", a.port), timeout=10)
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        sock.settimeout(None)
        log(f"connected to 127.0.0.1:{a.port} in netns {a.connect_netns or 'current'}")

        hello = {"protocol": 0, "session": a.session, "device_port": a.device_port}
        sock.sendall(encode(HELLO, json.dumps(hello, separators=(",", ":")).encode()))

        reader = FrameReader(self.mtu)
        sock.setblocking(False)
        os.set_blocking(tun, False)
        pending_out = bytearray()  # bytes queued for the socket
        tun_queue = []  # packets queued for the TUN (bounded)
        QUEUE = 256
        ready_file = a.ready_file
        running = True
        exit_code = 0

        def stop(*_):
            nonlocal running
            running = False

        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)

        def process_frames():
            """Decode buffered frames until the TUN queue is full. False = stop."""
            nonlocal running, exit_code
            try:
                for mtype, body in reader.frames():
                    if mtype == HELLO_ACK:
                        if self.state != "connected":
                            raise ProtocolError("HELLO_ACK out of state")
                        ack = json.loads(body)
                        if ack["protocol"] != 0:
                            raise ProtocolError("HELLO_ACK protocol")
                        self.mtu = reader.mtu = ack["mtu"]
                        self.state = "negotiated"
                        log(f"HELLO_ACK mtu={self.mtu}")
                    elif mtype == CONFIGURE_VPN:
                        if self.state != "negotiated":
                            raise ProtocolError("CONFIGURE_VPN out of state")
                        cfg = json.loads(body)
                        if cfg["mtu"] != self.mtu:
                            raise ProtocolError("CONFIGURE_VPN mtu mismatch")
                        log(f"CONFIGURE_VPN {cfg}")
                        ready = self.configure(cfg)
                        pending_out.extend(encode(VPN_READY, json.dumps(ready, separators=(",", ":")).encode()))
                        self.state = "active"
                        if ready_file:
                            with open(ready_file, "w") as f:
                                f.write("ready\n")
                        log("VPN_READY sent; Active")
                    elif mtype == IP_PACKET:
                        if self.state != "active":
                            raise ProtocolError("IP_PACKET before Active")
                        if not ipv4_ok(body):
                            raise ProtocolError("invalid IPv4 from host")
                        tun_queue.append(body)
                        if len(tun_queue) >= QUEUE:
                            return True  # bounded: leave the rest buffered, stop reading the socket
                    elif mtype == PING:
                        pending_out.extend(encode(PONG))
                    elif mtype == PONG:
                        pass
                    elif mtype == STOP:
                        log("STOP from host")
                        running = False
                        return False
                    elif mtype == ERROR:
                        log(f"ERROR from host: {body.decode(errors='replace')}")
                        running = False
                        exit_code = 2
                        return False
                    else:
                        raise ProtocolError(f"unexpected type 0x{mtype:02x}")
            except ProtocolError as e:
                log(f"protocol violation: {e}; closing")
                err = {"code": "protocol", "message": str(e)}
                try:
                    sock.setblocking(True)
                    sock.sendall(encode(ERROR, json.dumps(err).encode()))
                except OSError:
                    pass
                running = False
                exit_code = 2
                return False
            return True

        while running:
            if len(tun_queue) < QUEUE and reader.buf:
                if not process_frames():  # frames left buffered while the TUN queue was full
                    break
            rlist = []
            if len(tun_queue) < QUEUE:
                rlist.append(sock)  # suspend socket reads when the TUN queue is full
            if self.state == "active" and len(pending_out) < QUEUE * self.mtu:
                rlist.append(tun)  # suspend TUN reads when the socket queue is full
            wlist = []
            if pending_out:
                wlist.append(sock)
            if tun_queue:
                wlist.append(tun)
            try:
                r, w, _ = select.select(rlist, wlist, [], 1.0)
            except InterruptedError:
                continue

            if sock in r:
                try:
                    data = sock.recv(65536)
                except BlockingIOError:
                    data = None
                if data == b"":
                    log("host closed the connection")
                    running = False
                    break
                if data:
                    reader.feed(data)
                    if not process_frames():
                        break

            if tun in r:
                # Android VPN read -> IP_PACKET -> host.
                for _ in range(64):
                    try:
                        pkt = os.read(tun, 65536)
                    except BlockingIOError:
                        break
                    except InterruptedError:
                        continue  # zero bytes transferred: retry is allowed
                    if len(pkt) > self.mtu or not ipv4_ok(pkt):
                        self.stats["drop_invalid"] += 1
                        continue
                    pending_out += encode(IP_PACKET, pkt)
                    self.stats["tun_to_host"] += 1

            if sock in w and pending_out:
                try:
                    n = sock.send(pending_out)  # partial stream writes continue later
                    del pending_out[:n]
                except BlockingIOError:
                    pass
                except OSError as e:
                    log(f"socket write failed: {e}")
                    running = False

            if tun in w and tun_queue:
                pkt = tun_queue.pop(0)
                try:
                    n = os.write(tun, pkt)  # exactly one packet per write
                except InterruptedError:
                    tun_queue.insert(0, pkt)  # nothing transferred: retry allowed
                    continue
                except BlockingIOError:
                    tun_queue.insert(0, pkt)
                    continue
                if n != len(pkt):
                    log(f"short TUN write {n}/{len(pkt)}: terminating packet path")
                    running = False
                    exit_code = 3
                    break
                self.stats["host_to_tun"] += 1

        if exit_code == 0 and self.state == "active":
            try:
                sock.setblocking(True)
                sock.sendall(bytes(pending_out) + encode(STOP))
            except OSError:
                pass
        sock.close()
        os.close(tun)
        log(f"exit: stats={self.stats}")
        return exit_code


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--port", type=int, required=True, help="host loopback port")
    p.add_argument("--session", required=True)
    p.add_argument("--device-port", type=int, default=9000)
    p.add_argument("--tun-name", default="phonetun0")
    p.add_argument("--tun-netns", help="named netns (under /run/netns) in which to create the TUN")
    p.add_argument("--connect-netns", help="named netns in which to connect to 127.0.0.1:PORT")
    p.add_argument("--ready-file", help="file to create once VPN_READY has been sent")
    args = p.parse_args()
    sys.exit(FakeAndroid(args).run())


if __name__ == "__main__":
    main()
