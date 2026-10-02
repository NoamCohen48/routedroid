#!/usr/bin/env python3
"""Hostile/faulty host for the Android app (protocol/version-1.md negative cases).

Plays the host end over the real adb server: reverse mapping, bootstrap record,
`am start`, then misbehaves on purpose and reports how the app reacted.

    fake_host.py SERIAL all | CASE [CASE...]
"""
import hmac, hashlib, json, os, secrets, socket, struct, subprocess, sys, time

HELLO, HELLO_ACK, CONFIGURE_VPN, VPN_READY, VPN_ERROR, AUTH = 1, 2, 3, 4, 5, 6
IP_PACKET, PING, PONG, STOP, ERROR = 0x10, 0x20, 0x21, 0x30, 0x7F
NAMES = {1: "HELLO", 2: "HELLO_ACK", 3: "CONFIGURE_VPN", 4: "VPN_READY", 5: "VPN_ERROR", 6: "AUTH",
         0x10: "IP_PACKET", 0x20: "PING", 0x21: "PONG", 0x30: "STOP", 0x7F: "ERROR"}
MTU = 1400
DEVICE_PORT = 17900
SERIAL = None


def adb(*args, stdin=None):
    return subprocess.run(["adb", "-s", SERIAL, *args], input=stdin, capture_output=True, timeout=30)


def frame(t, body=b"", version=1, flags=0, length=None):
    return struct.pack(">IBBH", len(body) if length is None else length, version, t, flags) + body


def recv_exact(s, n):
    buf = b""
    while len(buf) < n:
        c = s.recv(n - len(buf))
        if not c:
            return None
        buf += c
    return buf


def recv_frame(s):
    h = recv_exact(s, 8)
    if h is None:
        return None
    n, v, t, f = struct.unpack(">IBBH", h)
    body = recv_exact(s, n) if n else b""
    return t, body


def transcript(session, port, cn, hn):
    return (b"routedroid-auth-v1\0" + bytes([1]) + session.encode() + b"\0" + struct.pack(">H", port)
            + b"android" + cn + b"host" + hn)


def proof(secret, role, t):
    return hmac.new(secret, role.encode() + t, hashlib.sha256).digest()


def record(session, secret, port=DEVICE_PORT, reserved=0):
    return b"RDB1" + bytes([1, reserved]) + struct.pack(">H", port) + session.encode().ljust(40, b"\0") + secret


def write_record(rec):
    r = adb("shell", "content", "write", "--uri", "content://dev.routedroid.bootstrap/record", stdin=rec)
    assert r.returncode == 0, r.stderr


def start(session):
    r = adb("shell", "am", "start", "-n", "dev.routedroid/.bootstrap.BootstrapActivity", "--es", "session", session)
    assert r.returncode == 0, r.stderr


def reverse(host_port):
    adb("reverse", "--remove", f"tcp:{DEVICE_PORT}")
    r = adb("reverse", f"tcp:{DEVICE_PORT}", f"tcp:{host_port}")
    assert r.returncode == 0, r.stderr


def bootstrap(session, secret, host_port):
    reverse(host_port)
    write_record(record(session, secret))
    start(session)


def listener(timeout):
    ls = socket.socket()
    ls.bind(("127.0.0.1", 0))
    ls.listen(1)
    ls.settimeout(timeout)
    return ls


def expect_no_connection(ls):
    try:
        ls.accept()
        raise AssertionError("app connected")
    except socket.timeout:
        pass
    finally:
        ls.close()


def vpn_up():
    out = adb("shell", "ip", "-4", "addr").stdout.decode()
    return "10.91.0.7" in out


def launch(app_secret, host_secret=None):
    """Bootstraps the app and returns (socket, session, secret, client_nonce) after HELLO."""
    session = secrets.token_hex(8)
    ls = listener(20)
    bootstrap(session, app_secret, ls.getsockname()[1])
    s, _ = ls.accept()
    ls.close()
    s.settimeout(15)
    t, body = recv_frame(s)
    assert t == HELLO, NAMES.get(t)
    hello = json.loads(body)
    assert hello["session"] == session and hello["device_port"] == DEVICE_PORT
    return s, session, (host_secret or app_secret), bytes.fromhex(hello["client_nonce"])


def handshake(s, session, secret, cn):
    hn = secrets.token_bytes(32)
    t = transcript(session, DEVICE_PORT, cn, hn)
    s.sendall(frame(HELLO_ACK, json.dumps({"protocol": 1, "mtu": MTU, "host_nonce": hn.hex(),
                                           "host_proof": proof(secret, "host", t).hex()}).encode()))
    ty, body = recv_frame(s)
    assert ty == AUTH, NAMES.get(ty)
    assert hmac.compare_digest(bytes.fromhex(json.loads(body)["android_proof"]), proof(secret, "android", t))


def configure(s, mtu=MTU):
    cfg = {"mtu": mtu, "addresses": [{"address": "10.91.0.7", "prefix": 32}],
           "routes": [{"address": "10.91.0.0", "prefix": 24}], "dns": [], "session_name": "fake"}
    s.sendall(frame(CONFIGURE_VPN, json.dumps(cfg).encode()))


def expect_vpn_error(s, code):
    r = recv_frame(s)
    assert r is not None, "app closed without VPN_ERROR"
    t, body = r
    assert t == VPN_ERROR, f"got {NAMES.get(t)}"
    e = json.loads(body)
    assert e["code"] == code, e
    assert recv_frame(s) is None, "socket still open after VPN_ERROR"
    return e


def expect_closed(s):
    s.settimeout(10)
    assert recv_frame(s) is None, "app kept the socket open"


# ---------------------------------------------------------------- cases

def case_wrong_secret():
    """§5 step 2: bad host_proof -> app closes silently, no AUTH, no VPN."""
    s, session, secret, cn = launch(secrets.token_bytes(32), host_secret=secrets.token_bytes(32))
    hn = secrets.token_bytes(32)
    t = transcript(session, DEVICE_PORT, cn, hn)
    s.sendall(frame(HELLO_ACK, json.dumps({"protocol": 1, "mtu": MTU, "host_nonce": hn.hex(),
                                           "host_proof": proof(secret, "host", t).hex()}).encode()))
    expect_closed(s)
    assert not vpn_up()


def case_no_record():
    """§7.2: launch without a record does nothing observable (no connection)."""
    ls = listener(6)
    reverse(ls.getsockname()[1])
    start("nope")
    expect_no_connection(ls)


def case_bad_record():
    """§7.1: a record with a nonzero reserved byte or port 0 is refused, so its launch does nothing."""
    for rec in (record("bad-reserved", secrets.token_bytes(32), reserved=1),
                record("bad-port", secrets.token_bytes(32), port=0)):
        ls = listener(5)
        reverse(ls.getsockname()[1])
        adb("shell", "content", "write", "--uri", "content://dev.routedroid.bootstrap/record", stdin=rec)  # refused
        start(rec[8:48].rstrip(b"\0").decode())
        expect_no_connection(ls)


def case_session_mismatch():
    """§7.2: a launch naming another session leaves the record in place for the right one."""
    session, secret = secrets.token_hex(8), secrets.token_bytes(32)
    ls = listener(5)
    reverse(ls.getsockname()[1])
    write_record(record(session, secret))
    start("someone-else")
    try:
        ls.accept()
        raise AssertionError("app connected for the wrong session")
    except socket.timeout:
        pass
    ls.settimeout(20)
    start(session)
    s, _ = ls.accept()
    ls.close()
    s.settimeout(15)
    t, body = recv_frame(s)
    assert t == HELLO and json.loads(body)["session"] == session
    s.close()


def case_superseded():
    """A new launch ends the active session: the old host gets VPN_ERROR internal, the new one HELLO."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s2, session2, secret2, cn2 = launch(secrets.token_bytes(32))
    expect_vpn_error(s, "internal")
    active(s2, session2, secret2, cn2)
    s2.sendall(frame(STOP))
    expect_closed(s2)


def case_bad_frame_negotiated():
    """Frame version 2 while Negotiated -> VPN_ERROR protocol_error, close."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    handshake(s, session, secret, cn)
    s.sendall(frame(CONFIGURE_VPN, b"{}", version=2))
    expect_vpn_error(s, "protocol_error")
    assert not vpn_up()


def case_huge_control():
    """Body length 0xFFFFFFFF must be rejected on the header, never allocated."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    handshake(s, session, secret, cn)
    s.sendall(frame(CONFIGURE_VPN, b"", length=0xFFFFFFFF))
    expect_vpn_error(s, "protocol_error")


def case_config_rejected():
    """CONFIGURE_VPN mtu != negotiated -> VPN_ERROR config_rejected."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    handshake(s, session, secret, cn)
    configure(s, mtu=1300)
    expect_vpn_error(s, "config_rejected")
    assert not vpn_up()


def case_config_malformed():
    """CONFIGURE_VPN with two addresses -> VPN_ERROR config_rejected."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    handshake(s, session, secret, cn)
    cfg = {"mtu": MTU, "addresses": [{"address": "10.91.0.7", "prefix": 32}, {"address": "10.91.0.8", "prefix": 32}],
           "routes": [{"address": "0.0.0.0", "prefix": 0}], "dns": [], "session_name": "fake"}
    s.sendall(frame(CONFIGURE_VPN, json.dumps(cfg).encode()))
    expect_vpn_error(s, "config_rejected")


def active(s, session, secret, cn):
    handshake(s, session, secret, cn)
    configure(s)
    t, body = recv_frame(s)
    assert t == VPN_READY, NAMES.get(t)
    r = json.loads(body)
    assert r["mtu"] == MTU and r["addresses"] == [{"address": "10.91.0.7", "prefix": 32}], r
    assert vpn_up()


def case_active_garbage():
    """Oversize IP_PACKET while Active -> VPN_ERROR protocol_error, socket closed, VPN gone."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s.sendall(frame(IP_PACKET, b"", length=100_000))
    expect_vpn_error(s, "protocol_error")
    time.sleep(1)
    assert not vpn_up(), "VPN still up after violation"


def case_active_out_of_state():
    """HELLO_ACK while Active -> VPN_ERROR protocol_error."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s.sendall(frame(HELLO_ACK, b"{}"))
    expect_vpn_error(s, "protocol_error")


def case_bad_ipv4_is_dropped():
    """IPv4 check failure is a drop, not a violation: session stays up, PING still answered."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s.sendall(frame(IP_PACKET, b"\x65" + b"\0" * 30))       # version 6
    s.sendall(frame(IP_PACKET, b"\x45\0\0\x10" + b"\0" * 27))  # total_length mismatch
    s.sendall(frame(PING))
    t, _ = recv_frame(s)
    assert t == PONG, NAMES.get(t)
    s.sendall(frame(STOP)); s.close()


def case_host_stop():
    """STOP while Active -> app closes, VPN gone, nothing sent back."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s.sendall(frame(STOP))
    expect_closed(s)
    time.sleep(1)
    assert not vpn_up()


def case_host_error():
    """ERROR while Active -> app closes, VPN gone."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s.sendall(frame(ERROR, json.dumps({"code": "internal", "message": "test"}).encode()))
    expect_closed(s)
    time.sleep(1)
    assert not vpn_up()


def case_keepalive_dead():
    """§5.1: app pings every 10 s of silence and closes after 30 s (slow)."""
    s, session, secret, cn = launch(secrets.token_bytes(32))
    active(s, session, secret, cn)
    s.settimeout(15)
    t0 = time.time()
    pings = 0
    while True:
        r = recv_frame(s)
        if r is None:
            break
        if r[0] == VPN_ERROR:  # §5.1: a silent host ends with internal
            assert json.loads(r[1])["code"] == "internal", r[1]
            assert recv_frame(s) is None, "socket still open after VPN_ERROR"
            break
        assert r[0] == PING, NAMES.get(r[0])
        pings += 1
    assert pings >= 2, f"{pings} PINGs in 30 s of silence"
    assert 28 < time.time() - t0 < 40, f"closed after {time.time() - t0:.0f}s"
    time.sleep(1)
    assert not vpn_up()


CASES = {k[5:]: v for k, v in globals().items() if k.startswith("case_")}

if __name__ == "__main__":
    SERIAL = sys.argv[1]
    subprocess.run([os.path.join(os.path.dirname(os.path.abspath(__file__)), "prepare-device.sh"), SERIAL], check=True)
    names = list(CASES) if sys.argv[2:] == ["all"] else sys.argv[2:]
    if "keepalive_dead" in names and sys.argv[2:] == ["all"] and os.environ.get("SLOW") != "1":
        names.remove("keepalive_dead")
    passed = failed = 0
    for n in names:
        try:
            CASES[n]()
            print(f"PASS  {n}"); passed += 1
        except Exception as e:
            print(f"FAIL  {n}: {type(e).__name__}: {e}"); failed += 1
        adb("reverse", "--remove", f"tcp:{DEVICE_PORT}")
        time.sleep(1)
    print(f"RESULT: {passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)
