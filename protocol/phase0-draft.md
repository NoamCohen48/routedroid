# Phase 0 Wire Protocol Draft

Throwaway-quality contract for the Phase 0 §3.1 minimal tunnel. Both the Rust host
probe and the Kotlin Android probe MUST implement exactly this. Supersedes nothing;
`protocol/version-1.md` will replace it in Phase 1.

## Transport

- Host binds `127.0.0.1:<HOST_PORT>` (loopback only).
- Host runs `adb -s SERIAL reverse tcp:<DEVICE_PORT> tcp:<HOST_PORT>`.
- Android connects to `127.0.0.1:<DEVICE_PORT>` and calls `VpnService.protect(socket)`
  before the VPN is established.
- One TCP stream carries both control frames and packet frames.

## Frame header (8 bytes, network byte order)

```
u32 body_length   // excludes header; 0 allowed only for PING/PONG/STOP
u8  version       // must be 0 for Phase 0
u8  message_type
u16 flags         // must be 0; nonzero -> close session
```

Limits: control bodies <= 65536 bytes; IP_PACKET bodies <= negotiated `mtu`
(default 1400) and > 20. Violation -> close the connection.

## Message types

| Value | Name          | Direction        | Body |
|-------|---------------|------------------|------|
| 0x01  | HELLO         | Android -> Host  | JSON |
| 0x02  | HELLO_ACK     | Host -> Android  | JSON |
| 0x03  | CONFIGURE_VPN | Host -> Android  | JSON |
| 0x04  | VPN_READY     | Android -> Host  | JSON |
| 0x05  | VPN_ERROR     | Android -> Host  | JSON |
| 0x10  | IP_PACKET     | both             | exactly one raw IPv4 packet, no PI header |
| 0x20  | PING          | both             | empty |
| 0x21  | PONG          | both             | empty |
| 0x30  | STOP          | both             | empty |
| 0x7F  | ERROR         | both             | JSON |

Phase 0 has NO authentication (Phase 0 §3.4 adds it later). State machine:

```
Connected --HELLO/HELLO_ACK--> Negotiated --CONFIGURE_VPN--> Configuring
Configuring --VPN_READY--> Active (IP_PACKET allowed both ways)
Configuring --VPN_ERROR--> Closed
any --STOP--> Closed
```

IP_PACKET before Active -> close.

## JSON bodies

HELLO:
```json
{"protocol":0,"session":"<opaque string from am start extra>","device_port":9000}
```
HELLO_ACK:
```json
{"protocol":0,"mtu":1400}
```
CONFIGURE_VPN:
```json
{
  "mtu": 1400,
  "addresses": [{"address":"192.168.10.74","prefix":32}],
  "routes":    [{"address":"0.0.0.0","prefix":0}],
  "dns":       ["192.168.10.1"],
  "session_name": "Routedroid Phase 0"
}
```
VPN_READY:
```json
{"addresses":["192.168.10.74/32"],"mtu":1400}
```
VPN_ERROR / ERROR:
```json
{"code":"vpn_permission_denied","message":"human readable"}
```

## Android launch (Phase 0 only; no secret yet)

```
adb -s SERIAL shell am start -n dev.routedroid.phase0/.BootstrapActivity \
    --es session <id> --ei device_port <DEVICE_PORT>
```

## IPv4 validation on both ends before injection

- version nibble == 4, IHL >= 5, total_length == body_length, total_length >= IHL*4.
