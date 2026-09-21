# 0003 — Host split into daemon and clients

Status: accepted, 2026-09-21. Applies from Phase 1.5 (before Phase 2).

## Decision

The host controller is a headless daemon, `routedroidd`, that owns every phone
session; user interfaces are separate processes that drive it over a control API:

```text
routedroid-phase0-helper   root, systemd system unit, socket-activated  (unchanged)
        ▲ SOCK_SEQPACKET (helper-ipc)
routedroidd                user, `systemd --user`, headless
        ▲ Unix SOCK_STREAM, JSON lines (routedroid-ipc), $XDG_RUNTIME_DIR/routedroid/control.sock
routedroid (CLI)  ·  routedroid-tui  ·  future tray/GUI
```

- `routedroidd` (crate `host/routedroidd`) holds the adb binding, one `AppListener`,
  `DeviceSession`, `HostNetwork` and `SessionDriver` per phone, a `HashMap<serial, SessionHandle>`,
  and an event bus. It applies all policy (`device::Transport`, one session per serial, unique
  phone address and TUN name). Clients cannot bypass what the CLI enforced before.
- `routedroid-ipc` (crate `host/routedroid-ipc`) is the API and the client: `Request`
  (`version`, `devices`, `start`, `stop`, `status`, `subscribe`), `Response`, `Event`
  (`session`, `traffic`, `devices`, `shutdown`), `SessionState`, `Outcome`, the fault taxonomy
  with its exit codes, and the default socket path. `API_VERSION` is bumped on incompatible
  change.
- `routedroid` is a thin CLI (`start` stays attached by default and stops the session on Ctrl-C,
  so scripts and the Phase 1 rig keep their shape; `--detach` returns at once).
- `routedroid-tui` is the first interactive client.

## Why

`architecture.md` §4.2 already lists `status --device` and `stop --device`; neither can exist
without a process that outlives one `start`. Multiple phones, a TUI, and Phase 2's per-interface
state (proxy ARP reference counts, sysctl restore) all need exactly one long-lived owner. Adding a
TUI inside the foreground CLI would have been a patch.

## Alternatives

- HTTP on loopback: needs an invented token; any local process (or a browser tab via DNS
  rebinding) can reach it. The Unix socket gets `0600` + `SO_PEERCRED` for free, the same
  pattern the helper uses.
- gRPC: protobuf tooling for six methods; not worth it while the API is this small. The
  transport (Unix socket) is the important half of the Docker analogy, not the encoding.
- CLI auto-starting the daemon (Docker style): racy and hides failures; the CLI prints
  `systemctl --user start routedroid` instead.

## Consequences

- Cleanup is unchanged: the helper still dies with its socket and `ExecStopPost` is the
  backstop; a daemon crash ends every session the same way one CLI crash did.
- Logs move to the daemon (`journalctl --user -u routedroid`); the CLI shows session state
  events, not the protocol log.
- Tests: `integration-tests/phase1/emulator-userns.sh` starts a daemon per run with a private
  socket; `fake_host.py` is unaffected (it drives the app directly).

## Known limit

The Phase 0 helper serves one session per activation, so the daemon can run sessions one after
another (systemd re-activates the helper) but not two phones at once yet. The daemon side is
ready for it (sessions keyed by serial, unique address and TUN per session); the helper gains
multi-session support in Phase 2.

## Out of scope for now

Multi-user, remote access, daemon-side persistence of sessions across restarts, socket
activation for the daemon.
