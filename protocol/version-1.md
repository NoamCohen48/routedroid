# Routedroid Wire Protocol, Version 1

Status: normative for version 1. Supersedes `phase0-draft.md`.

This document defines every byte the Routedroid host (Linux, Rust) and the
Routedroid Android app exchange. Both implementations MUST pass the golden
fixtures in `protocol/fixtures/` (see §10); a change to this document is a
change to those fixtures and to both implementations in the same commit.

The words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119.

## 1. Transport

- The host listens on `127.0.0.1:HOST_PORT` only. It MUST NOT bind any other
  address.
- The host maps the listener into the device with
  `adb -s SERIAL reverse tcp:DEVICE_PORT tcp:HOST_PORT`.
- The Android app connects to `127.0.0.1:DEVICE_PORT` (IPv4 loopback
  explicitly; adbd's reverse listener is IPv4-only on some builds) and calls
  `VpnService.protect()` on the socket before establishing the VPN.
- One TCP stream carries both control frames and packet frames, in both
  directions. There is no second connection.
- Version 1 accepts USB ADB only. The host MUST refuse a serial that denotes a
  network transport (`host:port`, `adb-…-…._adb-tls-connect._tcp`, or any
  serial containing `:`) with error `transport_unsupported` before doing
  anything else. Wireless debugging is deferred (decision record 0001).
- Confidentiality is provided by the transport (USB, or ADB's own TLS when
  wireless debugging is later enabled). This protocol provides
  authentication and integrity of the *session setup* only; packet payloads
  are not encrypted or authenticated by this protocol.

## 2. Frame

Every message is one frame: an 8-byte header followed by `body_length`
bytes. All multi-byte integers are big-endian.

```text
offset  size  field         rule
0       4     body_length   u32; bytes after the header
4       1     version       MUST be 1
5       1     message_type  see §3
6       2     flags         MUST be 0
8       n     body
```

Receivers MUST validate the header completely before allocating or reading
the body. A frame that fails any header rule is a protocol violation (§8).

Limits, checked on the header:

| message class | body_length |
|---|---|
| PING, PONG, STOP | exactly 0 |
| all other control messages | 1 … 65 536 |
| IP_PACKET | 20 … min(negotiated `mtu`, 65 535) |

`65 536` is the control limit (64 KiB). `65 535` is the absolute IPv4 total
length. The negotiated `mtu` is the value in HELLO_ACK (§4.2); before
HELLO_ACK has been sent or received IP_PACKET is not allowed in any case
(§5), so the limit is well defined whenever it is consulted.

## 3. Message types

| value | name | direction | body |
|---|---|---|---|
| 0x01 | HELLO | Android → host | JSON §4.1 |
| 0x02 | HELLO_ACK | host → Android | JSON §4.2 |
| 0x06 | AUTH | Android → host | JSON §4.3 |
| 0x03 | CONFIGURE_VPN | host → Android | JSON §4.4 |
| 0x04 | VPN_READY | Android → host | JSON §4.5 |
| 0x05 | VPN_ERROR | Android → host | JSON §4.6 |
| 0x10 | IP_PACKET | both | one raw IPv4 packet §6 |
| 0x20 | PING | both | empty |
| 0x21 | PONG | both | empty |
| 0x30 | STOP | both | empty |
| 0x7F | ERROR | host → Android | JSON §4.6 |

Any other value is a protocol violation. Values are listed in the order they
occur in a session; numbering is historical and MUST NOT be reassigned.

## 4. Control bodies

Control bodies are UTF-8 JSON objects (RFC 8259) without a byte-order mark.
Receivers MUST be strict: invalid UTF-8, a lone surrogate escape, trailing
bytes after the object, a duplicate member name, single quotes, unquoted
names, leading zeros, hex or octal numbers, and a number with a fraction or
exponent where an integer is required are all malformed bodies. A string is
never accepted where a number is required, nor the reverse. Lengths below
are counted in Unicode code points. `fixtures/bodies.json` pins these
cases. Field order
on the wire is not significant to receivers; senders SHOULD emit the order
shown so that fixtures are byte-stable. Receivers MUST ignore unknown
fields (forward compatibility) and MUST treat a missing or wrongly typed
required field as a protocol violation.

Hex strings are lowercase, without prefix, of exactly the stated length.

### 4.1 HELLO (Android → host)

```json
{"protocol":1,"session":"<id>","device_port":9000,"client_nonce":"<64 hex>","app":"<free text>"}
```

| field | required | rule |
|---|---|---|
| `protocol` | yes | integer; the highest version the app implements |
| `session` | yes | 1–40 characters from `A-Z a-z 0-9 . _ -`; the value the host passed at launch (§7) |
| `device_port` | yes | 1–65 535; the port the app connected to |
| `client_nonce` | yes | 32 random bytes from a CSPRNG, hex |
| `app` | no | app version string for diagnostics; ≤ 64 characters |

### 4.2 HELLO_ACK (host → Android)

```json
{"protocol":1,"mtu":1400,"host_nonce":"<64 hex>","host_proof":"<64 hex>"}
```

| field | required | rule |
|---|---|---|
| `protocol` | yes | MUST be 1 |
| `mtu` | yes | 576 … 65 535; the maximum IP_PACKET body for the rest of the session |
| `host_nonce` | yes | 32 random bytes from a CSPRNG, hex |
| `host_proof` | yes | §7.3, hex |

### 4.3 AUTH (Android → host)

```json
{"android_proof":"<64 hex>"}
```

### 4.4 CONFIGURE_VPN (host → Android)

```json
{
  "mtu": 1400,
  "addresses": [{"address":"10.100.102.222","prefix":32}],
  "routes":    [{"address":"0.0.0.0","prefix":0}],
  "dns":       ["10.100.102.1"],
  "session_name": "Routedroid"
}
```

| field | rule |
|---|---|
| `mtu` | MUST equal HELLO_ACK `mtu` |
| `addresses` | exactly one entry in version 1 (decision record 0001, gate 3): a unicast host address (below) with prefix 32 |
| `routes` | one or more; prefix 0–32 with every address bit past the prefix zero (`10.0.0.0/8`, not `10.0.0.1/8`) |
| `dns` | zero or more unicast host addresses |
| `session_name` | ≤ 64 characters; shown by Android in the VPN notification |

Addresses are dotted-quad IPv4: four decimal parts of 1–3 ASCII digits,
each 0–255, without leading zeros. A *unicast host address* is one outside
`0.0.0.0/8`, `127.0.0.0/8`, `224.0.0.0/4` and `240.0.0.0/4` (which holds the
limited broadcast address).

The app MUST reject (VPN_ERROR `config_rejected`) any value outside these
rules rather than pass it to `VpnService.Builder`.

### 4.5 VPN_READY (Android → host)

```json
{"addresses":[{"address":"10.100.102.222","prefix":32}],"mtu":1400}
```

| field | rule |
|---|---|
| `addresses` | one or more entries in CONFIGURE_VPN's shape, each a unicast host address with prefix 0–32: what was actually configured |
| `mtu` | MUST equal the negotiated value |

The host compares the parsed addresses with the ones it sent; anything
missing or extra is a protocol violation.

### 4.6 VPN_ERROR and ERROR

```json
{"code":"<snake_case>","message":"<human readable, ≤ 512 chars>"}
```

ERROR is host → Android only; the app reports everything, including a
protocol violation it detected, through VPN_ERROR. `code` values:

| code | sent by | meaning |
|---|---|---|
| `protocol_unsupported` | host | HELLO `protocol` is not 1; body MAY add `"supported":[1]` |
| `protocol_error` | either (host: ERROR, app: VPN_ERROR) | malformed frame or body, or message illegal in the current state |
| `auth_failed` | host | AUTH proof did not verify |
| `session_mismatch` | host | HELLO `session` or `device_port` is not the one the host launched |
| `transport_unsupported` | host | see §1 |
| `vpn_permission_denied` | Android (VPN_ERROR) | user declined the VPN consent |
| `vpn_establish_failed` | Android (VPN_ERROR) | `establish()` returned null or threw |
| `config_rejected` | Android (VPN_ERROR) | CONFIGURE_VPN failed §4.4 validation |
| `consent_timeout` | either (host: ERROR, app: VPN_ERROR) | the VPN was not ready within 120 s of AUTH (§5 step 5), normally because nobody answered the consent dialog |
| `internal` | either (host: ERROR, app: VPN_ERROR) | unexpected failure; message is diagnostic only |

A receiver MUST NOT act on `message` programmatically; it is for logs and
the user.

## 5. Session state machine

```text
                 HELLO ▸          ◂ HELLO_ACK        AUTH ▸ (verified)
  Connected ───────────► Authenticating ────────────────────► Negotiated
                                                                   │ ◂ CONFIGURE_VPN
                                                                   ▼
        Active ◄──────────────────────────────────────────── Configuring
          ▲   VPN_READY ▸                                          │ VPN_ERROR ▸
          │                                                        ▼
   IP_PACKET / PING / PONG both ways                             Closed
```

Legal *received* message types per state. Anything else received in that
state is a protocol violation.

| state | host may receive | Android may receive |
|---|---|---|
| Connected | HELLO, STOP | STOP |
| Authenticating | AUTH, STOP | HELLO_ACK, ERROR, STOP |
| Negotiated | STOP | CONFIGURE_VPN, ERROR, STOP |
| Configuring | VPN_READY, VPN_ERROR, STOP | ERROR, STOP |
| Active | IP_PACKET, PING, PONG, STOP, VPN_ERROR | IP_PACKET, PING, PONG, STOP, ERROR |

| Closed | — | — |

VPN_ERROR in Active means the app lost the VPN (revoked by the user or the
system), failed locally, or detected a violation; the host tears down.

Transitions:

1. Android sends HELLO immediately after connecting. Host validates it;
   on `protocol` ≠ 1 it sends ERROR `protocol_unsupported` and closes; on a
   session or port mismatch it sends ERROR `session_mismatch` and closes.
   Otherwise it sends HELLO_ACK. Both sides are now Authenticating.
2. Android verifies `host_proof` (§7.3). On failure it closes the socket
   without sending anything and discards the secret. On success it sends
   AUTH and discards the secret.
3. Host verifies `android_proof`. On failure it sends ERROR `auth_failed`,
   closes, and discards the secret. On success it discards the secret; both
   sides are Negotiated, and the host sends CONFIGURE_VPN at once
   (Configuring).
4. Android MUST NOT show the VPN consent dialog, start the VPN service, or
   persist anything before step 3 succeeded.
5. Android applies the configuration and sends VPN_READY (Active) or
   VPN_ERROR (Closed). The user may be answering the VPN consent dialog, so
   the host waits up to 120 seconds after AUTH for either; then it sends
   ERROR `consent_timeout` and closes. An app that is ready later than 120
   seconds after it sent AUTH MUST NOT establish the VPN: it sends VPN_ERROR
   `consent_timeout` and closes instead.
6. In Active either side sends IP_PACKET freely. Android's VPN stop, from
   any cause, MUST close the socket (which ends the packet path on both sides).
   Android ends the session with STOP when the user stopped it and with
   VPN_ERROR for every other cause it detected itself (revocation, a local
   failure, a violation, a dead peer).
7. STOP is legal in every state except Closed; a receiver of STOP closes
   the socket without reply. A sender of STOP or VPN_ERROR sends nothing
   after it and closes.

Only one session exists per socket; a second HELLO is a protocol violation.

### 5.1 Keepalive

In Active, a side that has neither sent nor received any frame for
10 seconds SHOULD send PING. A receiver of PING MUST reply PONG promptly
(target < 1 s). A side that sees no frame at all for 30 seconds SHOULD treat
the session as dead and close. PING and PONG are protocol violations outside
Active.

## 6. IP_PACKET body

Exactly one IPv4 packet, no packet-information header, no padding. Before
injecting a received packet into the TUN or VPN interface the receiver
MUST check:

- byte 0 high nibble (version) is 4;
- IHL (low nibble) ≥ 5;
- total length (bytes 2–3) equals `body_length`;
- total length ≥ IHL × 4.

A packet failing these checks is dropped and counted; it is not a protocol
violation (the peer may legitimately forward garbage it received). A
`body_length` outside §2 *is* a violation because the header already lies.

IPv6 is not carried in version 1.

## 7. Bootstrap and authentication

### 7.1 Secret delivery (host → app, out of band)

The host generates a 32-byte secret from a CSPRNG and streams an 80-byte
record to the app's bootstrap content provider through ADB standard input:

```text
adb -s SERIAL shell content write --uri content://dev.routedroid.bootstrap/record < record

record = "RDB1"[4] | version u8 = 1 | reserved u8 = 0 | device_port u16 | session[40] | secret[32]
```

`device_port` is big-endian, 1–65 535: the port the app connects to.
`session` is UTF-8, NUL-padded to 40 bytes, with nothing but NUL after the
first NUL. A record with a non-zero reserved byte is malformed. The secret
MUST NOT appear in a command argument, an intent extra, an environment
variable, a file, or a log on either side.

The provider MUST be exported, non-browsable, guarded by
`android.permission.DUMP`, and MUST additionally check
`Binder.getCallingUid() == 2000` (shell). It MUST return a write-only
descriptor and MUST NOT allow reads. Only the shell can write, so a new
record replaces (and wipes) a pending one: that is a host retrying. The
record is held only in app-process memory, is wiped 60 seconds after it
arrived, and is consumed (removed) by the first launch that names its
session.

### 7.2 Launch

```text
adb -s SERIAL shell am start -n dev.routedroid/.bootstrap.BootstrapActivity --es session <id>
```

The app connects to the record's `device_port`. Everything a launch carries
is untrusted, because any app on the phone can send it; the session id only
selects the record. A launch whose `session` does not match an unexpired
record MUST do nothing observable: no window, no connection, no VPN consent,
no service, no persisted state.

### 7.3 Mutual proof

```text
transcript    = "routedroid-auth-v1" 0x00
              | protocol u8 (= 1)
              | session utf8 | 0x00
              | device_port u16 big-endian
              | "android" | client_nonce[32]
              | "host"    | host_nonce[32]

host_proof    = HMAC-SHA256(secret, "host"    || transcript)
android_proof = HMAC-SHA256(secret, "android" || transcript)
```

Verification MUST use a constant-time comparison. The role labels make the
two proofs distinct, so a proof cannot be reflected; both nonces bind each
proof to this run; `session` and `device_port` bind it to this launch.

Both sides MUST wipe the secret from memory as soon as the AUTH step is
decided, whichever way it went, and MUST NOT reuse a secret for a second
connection.

## 8. Protocol violations

On any violation (bad header, bad body, illegal message for the state,
second HELLO, second session on the socket) the receiver:

1. MAY send ERROR `protocol_error` if it is the host and the socket is still
   writable;
2. MUST close the socket;
3. MUST tear down the VPN / TUN session if one was active;
4. MUST NOT attempt to resynchronise the stream.

## 9. Version compatibility

- `version` in the frame header and `protocol` in HELLO / HELLO_ACK are the
  same number and change together.
- A host that receives HELLO with an unknown `protocol` answers ERROR
  `protocol_unsupported` and closes **before** any VPN state exists on the
  device. The user sees "update the app" or "update the host".
- Minor, compatible additions within version 1 are made by adding optional
  JSON fields, which receivers ignore when unknown. Anything else is
  version 2.

## 10. Golden fixtures

`protocol/fixtures/` is generated by `protocol/tools/gen-fixtures.py` and
checked in. Both implementations load these files in their unit tests:

| file | contents |
|---|---|
| `frames.json` | valid frames (type, body, exact wire bytes) and invalid headers with the expected rejection |
| `auth.json` | transcript and proof vectors for given secret, nonces, session and port |
| `bootstrap.json` | bootstrap record bytes for a given session, port and secret, and malformed records |
| `bodies.json` | control bodies JSON libraries disagree on, each to accept or to reject |
| `states.json` | the §5 allowlist table |

Rejection codes used by `frames.json`:
`unsupported_version`, `nonzero_flags`, `unknown_type`,
`control_body_too_large`, `unexpected_body`, `empty_body`,
`packet_body_out_of_range`, `truncated`.
