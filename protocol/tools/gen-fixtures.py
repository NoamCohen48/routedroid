#!/usr/bin/env python3
"""Generate protocol/fixtures/*.json from protocol/version-1.md.

Deterministic: rerunning produces identical files. Uses only the standard
library so the fixtures do not depend on either implementation.
"""
import re
import hashlib
import hmac
import json
import struct
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "fixtures"
VERSION = 1
MTU = 1400
CONTROL_LIMIT = 65536

T = {
    "HELLO": 0x01, "HELLO_ACK": 0x02, "CONFIGURE_VPN": 0x03, "VPN_READY": 0x04,
    "VPN_ERROR": 0x05, "AUTH": 0x06, "IP_PACKET": 0x10, "PING": 0x20,
    "PONG": 0x21, "STOP": 0x30, "ERROR": 0x7F,
}


def header(length, version=VERSION, mtype=0, flags=0):
    return struct.pack(">IBBH", length, version, mtype, flags)


def frame(name, body: bytes):
    return header(len(body), mtype=T[name]) + body


def j(obj):
    return json.dumps(obj, separators=(",", ":")).encode()


def ipv4_checksum(b):
    s = sum(struct.unpack(">%dH" % (len(b) // 2), b))
    while s >> 16:
        s = (s & 0xFFFF) + (s >> 16)
    return (~s) & 0xFFFF


def icmp_echo(src, dst, ident=1, seq=1, payload=b""):
    icmp = struct.pack(">BBHHH", 8, 0, 0, ident, seq) + payload
    icmp = icmp[:2] + struct.pack(">H", ipv4_checksum(icmp)) + icmp[4:]
    total = 20 + len(icmp)
    hdr = struct.pack(">BBHHHBBH4s4s", 0x45, 0, total, 1, 0, 64, 1, 0,
                      bytes(map(int, src.split("."))), bytes(map(int, dst.split("."))))
    hdr = hdr[:10] + struct.pack(">H", ipv4_checksum(hdr)) + hdr[12:]
    return hdr + icmp


# ---------------------------------------------------------------- frames.json
valid = []


def add(name, mtype, body):
    valid.append({
        "name": name, "type": T[mtype], "body_hex": body.hex(),
        "wire_hex": frame(mtype, body).hex(),
    })


NONCE_C = bytes([0xAA] * 32)
NONCE_H = bytes([0xBB] * 32)
SECRET = bytes(range(32))
SESSION = "s1"
PORT = 9000


def transcript(session, port, cn, hn, protocol=VERSION):
    return (b"routedroid-auth-v1\x00" + bytes([protocol]) + session.encode() + b"\x00"
            + struct.pack(">H", port) + b"android" + cn + b"host" + hn)


def proof(secret, role, tr):
    return hmac.new(secret, role + tr, hashlib.sha256).digest()


tr = transcript(SESSION, PORT, NONCE_C, NONCE_H)
HOST_PROOF = proof(SECRET, b"host", tr)
ANDROID_PROOF = proof(SECRET, b"android", tr)

add("hello", "HELLO", j({"protocol": 1, "session": SESSION, "device_port": PORT,
                         "client_nonce": NONCE_C.hex(), "app": "routedroid-android 1.0"}))
add("hello_minimal", "HELLO", j({"protocol": 1, "session": "abc", "device_port": 9000,
                                 "client_nonce": NONCE_C.hex()}))
add("hello_ack", "HELLO_ACK", j({"protocol": 1, "mtu": MTU, "host_nonce": NONCE_H.hex(),
                                 "host_proof": HOST_PROOF.hex()}))
add("auth", "AUTH", j({"android_proof": ANDROID_PROOF.hex()}))
add("configure_vpn", "CONFIGURE_VPN", j({
    "mtu": MTU,
    "addresses": [{"address": "10.100.102.222", "prefix": 32}],
    "routes": [{"address": "0.0.0.0", "prefix": 0}],
    "dns": ["10.100.102.1"],
    "session_name": "Routedroid",
}))
add("vpn_ready", "VPN_READY", j({"addresses": ["10.100.102.222/32"], "mtu": MTU}))
add("vpn_error", "VPN_ERROR", j({"code": "vpn_permission_denied", "message": "user declined"}))
add("error_auth_failed", "ERROR", j({"code": "auth_failed", "message": "proof mismatch"}))
add("error_protocol_unsupported", "ERROR",
    j({"code": "protocol_unsupported", "message": "host speaks 1", "supported": [1]}))
add("ping", "PING", b"")
add("pong", "PONG", b"")
add("stop", "STOP", b"")
PKT = icmp_echo("10.100.102.5", "10.100.102.222")
add("ip_packet_icmp_echo", "IP_PACKET", PKT)
# 20 bytes: a bare IPv4 header (protocol 59, no next header); the smallest legal body.
add("ip_packet_min", "IP_PACKET",
    bytes([0x45, 0, 0, 20, 0, 0, 0, 0, 64, 59, 0, 0, 10, 0, 0, 2, 10, 0, 0, 1]))
add("ip_packet_mtu", "IP_PACKET", icmp_echo("10.0.0.2", "10.0.0.1", payload=b"\x00" * (MTU - 28)))
add("control_body_at_limit", "ERROR", b"{" + b" " * (CONTROL_LIMIT - 2) + b"}")

hello_wire = frame("HELLO", j({"protocol": 1, "session": "abc", "device_port": 9000,
                                "client_nonce": NONCE_C.hex()}))

invalid = [
    {"name": "version_0", "wire_hex": (header(3, version=0, mtype=T["HELLO"]) + b"{}\n").hex(),
     "error": "unsupported_version"},
    {"name": "version_2", "wire_hex": (header(3, version=2, mtype=T["HELLO"]) + b"{}\n").hex(),
     "error": "unsupported_version"},
    {"name": "flags_low_bit", "wire_hex": (header(3, mtype=T["HELLO"], flags=1) + b"{}\n").hex(),
     "error": "nonzero_flags"},
    {"name": "flags_high_bit", "wire_hex": (header(3, mtype=T["HELLO"], flags=0x8000) + b"{}\n").hex(),
     "error": "nonzero_flags"},
    {"name": "unknown_type_0x11", "wire_hex": (header(3, mtype=0x11) + b"{}\n").hex(),
     "error": "unknown_type"},
    {"name": "unknown_type_0x00", "wire_hex": (header(3, mtype=0x00) + b"{}\n").hex(),
     "error": "unknown_type"},
    {"name": "unknown_type_0xff", "wire_hex": (header(3, mtype=0xFF) + b"{}\n").hex(),
     "error": "unknown_type"},
    {"name": "control_hostile_length", "wire_hex": header(0xFFFFFFFF, mtype=T["HELLO"]).hex(),
     "error": "control_body_too_large"},
    {"name": "control_one_over_limit", "wire_hex": header(CONTROL_LIMIT + 1, mtype=T["HELLO"]).hex(),
     "error": "control_body_too_large"},
    {"name": "ping_with_body", "wire_hex": (header(1, mtype=T["PING"]) + b"\xaa").hex(),
     "error": "unexpected_body"},
    {"name": "stop_with_body", "wire_hex": (header(2, mtype=T["STOP"]) + b"\xaa\xbb").hex(),
     "error": "unexpected_body"},
    {"name": "hello_empty", "wire_hex": header(0, mtype=T["HELLO"]).hex(), "error": "empty_body"},
    {"name": "error_empty", "wire_hex": header(0, mtype=T["ERROR"]).hex(), "error": "empty_body"},
    {"name": "packet_empty", "wire_hex": header(0, mtype=T["IP_PACKET"]).hex(),
     "error": "packet_body_out_of_range"},
    {"name": "packet_19_bytes", "wire_hex": (header(19, mtype=T["IP_PACKET"]) + bytes(19)).hex(),
     "error": "packet_body_out_of_range"},
    {"name": "packet_mtu_plus_one", "wire_hex": header(MTU + 1, mtype=T["IP_PACKET"]).hex(),
     "error": "packet_body_out_of_range"},
    {"name": "packet_hostile_length", "wire_hex": header(0xFFFFFFFF, mtype=T["IP_PACKET"]).hex(),
     "error": "packet_body_out_of_range"},
    {"name": "truncated_header", "wire_hex": hello_wire[:5].hex(), "error": "truncated"},
    {"name": "truncated_body", "wire_hex": hello_wire[:-1].hex(), "error": "truncated"},
    {"name": "empty_stream", "wire_hex": "", "error": "truncated"},
]

frames = {
    "_comment": "Generated by protocol/tools/gen-fixtures.py from protocol/version-1.md. Do not edit.",
    "version": VERSION,
    "mtu": MTU,
    "control_limit": CONTROL_LIMIT,
    "valid": valid,
    "invalid": invalid,
}

# ------------------------------------------------------------------ auth.json
vectors = []
for name, secret, sess, port, cn, hn in [
    ("pinned", SECRET, SESSION, PORT, NONCE_C, NONCE_H),
    ("zero_secret", bytes(32), "session-with-dots.and_underscores-40char", 65535,
     bytes([0x01]) * 32, bytes([0x02]) * 32),
    ("port_1", bytes([0xFF]) * 32, "x", 1, bytes(range(32)), bytes(range(32, 64))),
]:
    assert re.fullmatch(r"[A-Za-z0-9._-]{1,40}", sess), sess
    t = transcript(sess, port, cn, hn)
    vectors.append({
        "name": name, "secret_hex": secret.hex(), "session": sess, "device_port": port,
        "client_nonce_hex": cn.hex(), "host_nonce_hex": hn.hex(),
        "transcript_hex": t.hex(),
        "host_proof_hex": proof(secret, b"host", t).hex(),
        "android_proof_hex": proof(secret, b"android", t).hex(),
    })
auth = {
    "_comment": frames["_comment"],
    "domain": "routedroid-auth-v1",
    "vectors": vectors,
}

# ------------------------------------------------------------- bootstrap.json
def record(session, secret, port=PORT, reserved=0):
    s = session.encode()
    assert 1 <= len(s) <= 40
    return b"RDB1" + bytes([VERSION, reserved]) + struct.pack(">H", port) + s.ljust(40, b"\x00") + secret

bootstrap = {
    "_comment": frames["_comment"],
    "magic": "RDB1",
    "length": 80,
    "provider_uri": "content://dev.routedroid.bootstrap/record",
    "vectors": [
        {"name": "pinned", "session": SESSION, "device_port": PORT, "secret_hex": SECRET.hex(),
         "record_hex": record(SESSION, SECRET).hex()},
        {"name": "max_session", "session": "s" * 40, "device_port": 65535, "secret_hex": (b"\xcd" * 32).hex(),
         "record_hex": record("s" * 40, b"\xcd" * 32, port=65535).hex()},
        {"name": "port_1", "session": "x", "device_port": 1, "secret_hex": bytes(32).hex(),
         "record_hex": record("x", bytes(32), port=1).hex()},
    ],
    "invalid": [
        {"name": "bad_magic", "record_hex": (b"RDB0" + record(SESSION, SECRET)[4:]).hex()},
        {"name": "bad_version", "record_hex": (b"RDB1\x00" + record(SESSION, SECRET)[5:]).hex()},
        {"name": "reserved_nonzero", "record_hex": record(SESSION, SECRET, reserved=1).hex()},
        {"name": "port_zero", "record_hex": record(SESSION, SECRET, port=0).hex()},
        {"name": "short", "record_hex": record(SESSION, SECRET)[:79].hex()},
        {"name": "long", "record_hex": (record(SESSION, SECRET) + b"\x00").hex()},
        {"name": "empty_session", "record_hex": (record(SESSION, SECRET)[:8] + bytes(40) + SECRET).hex()},
        {"name": "session_bad_char", "record_hex": record("a b", SECRET).hex()},
        {"name": "session_byte_after_nul",
         "record_hex": (record(SESSION, SECRET)[:8] + b"s1\x00x".ljust(40, b"\x00") + SECRET).hex()},
    ],
}

# --------------------------------------------------------------- bodies.json
# Bodies that one JSON library accepts and another rejects are where two
# implementations silently diverge, so both must agree on every case here.
CFG = {"mtu": MTU, "addresses": [{"address": "10.100.102.222", "prefix": 32}],
       "routes": [{"address": "0.0.0.0", "prefix": 0}], "dns": ["10.100.102.1"],
       "session_name": "Routedroid"}


def cfg(**kw):
    return j({**CFG, **kw})


def cfg_text(text):
    """CONFIGURE_VPN with the mtu member written by hand."""
    return j(CFG).replace(b'"mtu":1400', b'"mtu":' + text.encode())


def addr(a, p=32):
    return [{"address": a, "prefix": p}]


EMOJI = "\U0001F600"
ACK = {"protocol": 1, "mtu": MTU, "host_nonce": NONCE_H.hex(), "host_proof": HOST_PROOF.hex()}
bodies = {
    "_comment": frames["_comment"],
    "mtu": MTU,
    "valid": [
        {"name": "unknown_fields_ignored", "kind": "configure_vpn", "body_hex": cfg(future={"x": [1, 2]}).hex()},
        {"name": "whitespace_between_tokens", "kind": "configure_vpn",
         "body_hex": json.dumps(CFG, indent=2).encode().hex()},
        {"name": "escaped_slash_and_unicode", "kind": "configure_vpn",
         "body_hex": cfg().replace(b'"Routedroid"', b'"Route\\/droid \\u00e9"').hex()},
        {"name": "session_name_64_code_points", "kind": "configure_vpn",
         "body_hex": cfg(session_name=EMOJI * 64).hex()},
        {"name": "session_name_raw_utf8", "kind": "configure_vpn",
         "body_hex": json.dumps({**CFG, "session_name": "Caf\u00e9 " + EMOJI}, ensure_ascii=False,
                                separators=(",", ":")).encode().hex()},
        {"name": "route_host_prefix", "kind": "configure_vpn",
         "body_hex": cfg(routes=addr("192.168.7.9")).hex()},
        {"name": "no_dns", "kind": "configure_vpn", "body_hex": cfg(dns=[]).hex()},
        {"name": "message_512_code_points", "kind": "error",
         "body_hex": j({"code": "internal", "message": EMOJI * 512}).hex()},
        {"name": "unknown_error_code", "kind": "error",
         "body_hex": j({"code": "from_the_future", "message": ""}).hex()},
    ],
    "invalid": [
        # JSON syntax: serde_json is strict RFC 8259 and so must the app be.
        {"name": "trailing_garbage", "kind": "configure_vpn", "body_hex": (cfg() + b"x").hex()},
        {"name": "trailing_second_object", "kind": "configure_vpn", "body_hex": (cfg() + b"{}").hex()},
        {"name": "duplicate_key", "kind": "configure_vpn",
         "body_hex": cfg().replace(b'{"mtu":1400', b'{"mtu":1400,"mtu":1400').hex()},
        {"name": "single_quotes", "kind": "configure_vpn",
         "body_hex": cfg().replace(b'"Routedroid"', b"'Routedroid'").hex()},
        {"name": "unquoted_key", "kind": "configure_vpn", "body_hex": cfg().replace(b'"mtu"', b"mtu").hex()},
        {"name": "hex_integer", "kind": "configure_vpn", "body_hex": cfg_text("0x578").hex()},
        {"name": "leading_zero_integer", "kind": "configure_vpn", "body_hex": cfg_text("01400").hex()},
        {"name": "fraction_integer", "kind": "configure_vpn", "body_hex": cfg_text("1400.0").hex()},
        {"name": "exponent_integer", "kind": "configure_vpn", "body_hex": cfg_text("14e2").hex()},
        {"name": "string_integer", "kind": "configure_vpn", "body_hex": cfg(mtu="1400").hex()},
        {"name": "byte_order_mark", "kind": "configure_vpn", "body_hex": (b"\xef\xbb\xbf" + cfg()).hex()},
        {"name": "invalid_utf8", "kind": "configure_vpn",
         "body_hex": cfg().replace(b"Routedroid", b"Route\xffdroid").hex()},
        {"name": "lone_surrogate_escape", "kind": "configure_vpn",
         "body_hex": cfg().replace(b'"Routedroid"', b'"\\ud800"').hex()},
        {"name": "raw_control_character", "kind": "configure_vpn",
         "body_hex": cfg().replace(b"Routedroid", b"Route\x01droid").hex()},
        {"name": "not_an_object", "kind": "configure_vpn", "body_hex": b"[1,2]".hex()},
        # Types: no coercion between strings and numbers.
        {"name": "dns_number_element", "kind": "configure_vpn", "body_hex": cfg(dns=[7]).hex()},
        {"name": "address_object_is_string", "kind": "configure_vpn", "body_hex": cfg(addresses=["10.0.0.2/32"]).hex()},
        {"name": "supported_string_element", "kind": "error",
         "body_hex": j({"code": "protocol_unsupported", "message": "", "supported": ["1"]}).hex()},
        {"name": "supported_fraction_element", "kind": "error",
         "body_hex": j({"code": "protocol_unsupported", "message": "", "supported": [1.5]}).hex()},
        {"name": "supported_over_u8", "kind": "error",
         "body_hex": j({"code": "protocol_unsupported", "message": "", "supported": [256]}).hex()},
        {"name": "hello_ack_fraction_protocol", "kind": "hello_ack",
         "body_hex": j(ACK).replace(b'"protocol":1', b'"protocol":1.0').hex()},
        # Dotted quads: ASCII digits, no leading zeros, exactly four parts.
        {"name": "address_non_ascii_digits", "kind": "configure_vpn",
         "body_hex": cfg(addresses=addr("\u0661\u0660.0.0.1")).hex()},
        {"name": "address_leading_zero", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("010.0.0.1")).hex()},
        {"name": "address_three_parts", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("10.0.1")).hex()},
        {"name": "address_octet_256", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("10.0.0.256")).hex()},
        # Semantics (§4.4): the phone's own address is one unicast /32.
        {"name": "address_prefix_31", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("10.0.0.2", 31)).hex()},
        {"name": "address_unspecified", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("0.0.0.0")).hex()},
        {"name": "address_loopback", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("127.0.0.1")).hex()},
        {"name": "address_multicast", "kind": "configure_vpn", "body_hex": cfg(addresses=addr("224.0.0.1")).hex()},
        {"name": "address_broadcast", "kind": "configure_vpn",
         "body_hex": cfg(addresses=addr("255.255.255.255")).hex()},
        {"name": "two_addresses", "kind": "configure_vpn",
         "body_hex": cfg(addresses=addr("10.0.0.2") + addr("10.0.0.3")).hex()},
        {"name": "route_not_canonical", "kind": "configure_vpn", "body_hex": cfg(routes=addr("10.0.0.1", 8)).hex()},
        {"name": "route_prefix_33", "kind": "configure_vpn", "body_hex": cfg(routes=addr("10.0.0.1", 33)).hex()},
        {"name": "no_routes", "kind": "configure_vpn", "body_hex": cfg(routes=[]).hex()},
        {"name": "dns_unspecified", "kind": "configure_vpn", "body_hex": cfg(dns=["0.0.0.0"]).hex()},
        {"name": "dns_multicast", "kind": "configure_vpn", "body_hex": cfg(dns=["239.1.1.1"]).hex()},
        {"name": "mtu_mismatch", "kind": "configure_vpn", "body_hex": cfg(mtu=1500).hex()},
        {"name": "session_name_65_code_points", "kind": "configure_vpn",
         "body_hex": cfg(session_name=EMOJI * 65).hex()},
        {"name": "message_513_code_points", "kind": "error",
         "body_hex": j({"code": "internal", "message": EMOJI * 513}).hex()},
        {"name": "code_not_snake_case", "kind": "error", "body_hex": j({"code": "Internal", "message": ""}).hex()},
    ],
}

# ---------------------------------------------------------------- states.json
states = {
    "_comment": frames["_comment"],
    "states": ["Connected", "Authenticating", "Negotiated", "Configuring", "Active", "Closed"],
    "host_receives": {
        "Connected": ["HELLO", "STOP"],
        "Authenticating": ["AUTH", "STOP"],
        "Negotiated": ["STOP"],
        "Configuring": ["VPN_READY", "VPN_ERROR", "STOP"],
        "Active": ["IP_PACKET", "PING", "PONG", "STOP", "VPN_ERROR"],
        "Closed": [],
    },
    "android_receives": {
        "Connected": ["STOP"],
        "Authenticating": ["HELLO_ACK", "ERROR", "STOP"],
        "Negotiated": ["CONFIGURE_VPN", "ERROR", "STOP"],
        "Configuring": ["ERROR", "STOP"],
        "Active": ["IP_PACKET", "PING", "PONG", "STOP", "ERROR"],
        "Closed": [],
    },
    "types": T,
}

OUT.mkdir(parents=True, exist_ok=True)
for name, data in [("frames", frames), ("auth", auth), ("bootstrap", bootstrap), ("bodies", bodies), ("states", states)]:
    (OUT / f"{name}.json").write_text(json.dumps(data, indent=1) + "\n")
    print("wrote", OUT / f"{name}.json")
