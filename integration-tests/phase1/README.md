# Phase 1 acceptance (implementation-plan §4)

Two rigs, both without root, both against the real app on a real adb server:

- `emulator-userns.sh [SERIAL] [sigint|app]` — full `routedroid start` session. The
  helper runs in an unprivileged user+network namespace (dummy `lan0` 10.90.0.1/24 and the
  TUN live there); the CLI and adb stay in the host namespace; the helper socket is a Unix
  path so it crosses the boundary. Taps the consent dialogs, checks ICMP both ways, 200 KB
  TCP into `toybox nc` on the phone, ends the session (Ctrl-C on the host or the app's Stop
  button) and verifies both sides tore down.
- `fake_host.py SERIAL all|CASE...` — a misbehaving host in Python (stdlib only): does the
  reverse/record/launch dance itself, then breaks the protocol on purpose and checks the
  app's reaction (`SLOW=1` adds the 30 s keepalive case).

Prerequisites: `cargo build --release -p routedroid -p phase0-helper`, the app installed
(`android/app/build/outputs/apk/debug/app-debug.apk`), `socat`, `unshare`/`nsenter`.

## Results, 2026-09-21

| Acceptance item | Emulator, Android 14 (API 34) | Samsung SM-J810G, Android 10, USB |
|---|---|---|
| One statically configured address end to end (`routedroid start … --phone-ip 10.90.0.7`) | PASS | PASS |
| ICMP both directions, TCP PC → phone 200 KB md5-equal | PASS | PASS |
| Host Ctrl-C → STOP → app "session ended cleanly", VPN address gone | PASS | PASS |
| App Stop button → host "peer sent STOP", exit 0 | PASS | not run |
| Helper acknowledges Stop, TUN gone, reverse mapping removed | PASS | PASS |
| Both sides pass the same fixtures | `cargo test -p routedroid-proto`, `./gradlew :protocol:testDebugUnitTest` | — |
| Hostile app: forged record write, read, launch without record | all denied, no service (`:hostile`) | not run |

`fake_host.py` (app reaction; "closed" = socket closed, no VPN address on the phone):

| case | expected | emulator | Samsung |
|---|---|---|---|
| `wrong_secret` | closes silently after HELLO_ACK, no AUTH, no VPN | PASS | PASS |
| `no_record` | launch without record: never connects | PASS | PASS |
| `bad_frame_negotiated` (header version 2) | VPN_ERROR `protocol_error`, closed | PASS | PASS |
| `huge_control` (length 0xFFFFFFFF) | rejected on the header, VPN_ERROR `protocol_error` | PASS | PASS |
| `config_rejected` (mtu ≠ negotiated) | VPN_ERROR `config_rejected` | PASS | PASS |
| `config_malformed` (two addresses) | VPN_ERROR `config_rejected` | PASS | PASS |
| `active_garbage` (IP_PACKET length 100 000) | VPN_ERROR `protocol_error`, VPN torn down | PASS | PASS |
| `active_out_of_state` (HELLO_ACK while Active) | VPN_ERROR `protocol_error` | PASS | PASS |
| `bad_ipv4_is_dropped` | dropped, session stays up, PING answered | PASS | PASS |
| `host_stop`, `host_error` | closed, VPN gone | PASS | PASS |
| `keepalive_dead` (host silent) | app PINGs, closes after 30 s | PASS | not run |

Notes:

- The app rate-limits bootstrap launches (3 per 10 s); the negative runner sleeps 4 s
  between cases for that reason. The first run tripped the limit, which is the intended
  behaviour.
- Found and fixed during the run: the host's helper relay task swallowed the helper's
  `Stopped` reply, so `routedroid start` logged "helper did not acknowledge Stop" although
  cleanup had happened. Control replies now go through the relay to `stop()`.
- The Phase 0 helper (`phase0-helper`) is still the privileged side; Phase 2 replaces it.
