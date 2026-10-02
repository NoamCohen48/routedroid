# Emulator end-to-end rigs

All rigs run without root against the installed app over the real adb server. Each one first
runs `prepare-device.sh SERIAL`, which:

- grants VPN consent with appops;
- grants the notification permission on API 33+;
- clears leftover dialogs.

No rig therefore waits for a tap.

| Rig | What it does |
|---|---|
| `userns.sh [SERIAL] [sigint\|app\|early]` | Runs a full `routedroid start` session, details below. `SQUAT=N` adds N silent local connections that take the app port first; the app must still get through. |
| `fake_host.py SERIAL all\|CASE...` | A misbehaving host in Python (stdlib only). It does the reverse/record/launch steps itself, then breaks the protocol on purpose and checks how the app reacts. `SLOW=1` adds the 30 s keepalive case. |
| `hostile.sh SERIAL` | Builds and installs `android/testing/hostile`, lets it attack the exported surface, then checks the real host can still start a session. Each probe reports PASS, FAIL or INCONCLUSIVE, and anything but PASS fails the run. |

How `userns.sh` sets up and checks a session:

- **Setup:**
  - The helper runs in an unprivileged user and network namespace that holds a dummy
    `lan0` (10.90.0.1/24) and the TUN.
  - `routedroidd`, the CLI and adb stay in the host namespace.
  - The helper socket is a Unix path, so it crosses the namespace boundary.
- **Traffic checks:** ICMP both ways, 200 KB of TCP from PC to phone, and 2 MB of TCP from
  phone to PC.
- **Ending the session:** Ctrl-C on the host, the app's Stop button, or Ctrl-C right after
  launch (`early`).
- **Teardown checks:** both sides tore down. The app check reads `DeviceLink: session ended:
  UserStopped|HostStopped` from logcat.

Prerequisites:

- the release binaries: `cargo build --release -p routedroid -p routedroidd -p routedroid-helper`
  in `host/`;
- the app installed: `android/app/build/outputs/apk/debug/app-debug.apk`;
- `socat`, `unshare` and `nsenter`.

## Results, 2026-10-02 (protocol v1 with the port in the record; rebuilt app)

| Check | Emulator, Android 14 (API 34) | Samsung SM-J810G, Android 10, USB |
|---|---|---|
| `userns.sh sigint` (13 checks) | PASS | PASS |
| `userns.sh app` (14 checks: Stop button → host sees STOP) | PASS | PASS |
| `userns.sh early` (8 checks) | PASS | PASS |
| `SQUAT=3 userns.sh sigint` | PASS | not run |
| `hostile.sh` | PASS | PASS |
| `cargo test --workspace`, `./gradlew :protocol:test :app:testDebugUnitTest` | PASS | — |

`fake_host.py` cases. "Closed" means the socket closed with no VPN on the phone:

| Case | Expected | Emulator | Samsung |
|---|---|---|---|
| `wrong_secret` | closes silently after HELLO_ACK, no AUTH, no VPN | PASS | PASS |
| `no_record` | launch without a record never connects | PASS | PASS |
| `bad_record` | reserved byte ≠ 0 or port 0: record refused, launch never connects | PASS | PASS |
| `session_mismatch` | launch for another session never connects; the record still serves the right one | PASS | PASS |
| `superseded` | a new launch ends the active session with VPN_ERROR `internal`; the new one comes up | PASS | PASS |
| `bad_frame_negotiated` (header version 2) | VPN_ERROR `protocol_error`, closed | PASS | PASS |
| `huge_control` (length 0xFFFFFFFF) | rejected on the header, VPN_ERROR `protocol_error` | PASS | PASS |
| `config_rejected` (mtu ≠ negotiated) | VPN_ERROR `config_rejected` | PASS | PASS |
| `config_malformed` (two addresses) | VPN_ERROR `config_rejected` | PASS | PASS |
| `active_garbage` (IP_PACKET length 100 000) | VPN_ERROR `protocol_error`, VPN torn down | PASS | PASS |
| `active_out_of_state` (HELLO_ACK while Active) | VPN_ERROR `protocol_error` | PASS | PASS |
| `bad_ipv4_is_dropped` | dropped, session stays up, PING answered | PASS | PASS |
| `host_stop`, `host_error` | closed, nothing sent back, VPN gone | PASS | PASS |
| `keepalive_dead` (host silent) | PING every 10 s, VPN_ERROR `internal` and close at 30 s | PASS | PASS |

`hostile.sh` probes. All of them must be denied or have no effect:

| Probe | Result |
|---|---|
| Forged, well-formed record written to the provider | `SecurityException` (DUMP) |
| Provider opened in modes `r` and `rw` | `SecurityException` |
| `ContentResolver.call()` on the provider | `SecurityException` |
| `startService(ACTION_STOP)` | `SecurityException` (BIND_VPN_SERVICE) |
| 24 launches with no, garbage or guessed sessions | no VPN; the real host starts right after |

Notes:

- `keepalive_dead` found a bug in this run: after asking for a PING, the keepalive thread
  slept until the 30 s dead deadline instead of the next idle interval, so it sent only one
  PING. Fixed, with a regression test in `KeepaliveTest`.
- The old per-launch rate limit (3 per 10 s) is gone: it let any app lock the host out. The
  runner no longer sleeps between cases for it.
- `toybox nc` on the Android 14 emulator truncates a regular-file stdin when it is the client
  (8 KiB on loopback). The phone-to-PC bulk check therefore makes the phone the *listener*
  that sends the file.
