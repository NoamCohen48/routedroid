# phase0-helper — helper gate spike

Proves the architecture §5.3 shape before Phase 1 builds the real daemon:

- **privileged, per-session, socket-activated** systemd service (`Restart=no`);
- **typed requests only** over a Unix `SOCK_SEQPACKET` socket owned by group
  `routedroid` (`Start{lan_if, phone_ip, tun, mtu}`, `Stop`, `Ping`); the helper
  derives the host address/prefix itself and validates every name;
- **exclusive TUN ownership** with whole-packet relay to the controller over the
  same seqpacket connection (`[0x10][IPv4 packet]`);
- **write-ahead journal** (`/var/lib/routedroid/phase0-journal/<session>.journal`):
  `pending` → apply → `done`, and `undo_pending` → undo → `undone`, each line
  fsynced before the next step;
- **independent cleanup**: `ExecStopPost=phase0-helper cleanup` replays any
  unresolved journal after *any* exit (including SIGKILL); `ExecStartPre=
  phase0-helper check` refuses a new session while one is unresolved;
- **controller death** closes the socket → the helper undoes everything and exits.

Mutations per session, in order (deny-first): TUN, nftables table
`inet routedroid_p0`, `forwarding` on the TUN and LAN interface, `proxy_arp`
on the LAN interface, `/32` route to the phone via the TUN.

## Run

```sh
cargo build --release -p phase0-helper
sudo host/phase0-helper/install.sh              # units + group; re-login or use `sg routedroid`
phase0-helper client --lan-if eno1 --phone-ip 10.100.102.222 --bench 2000 --hold 5
sudo host/phase0-helper/install.sh --uninstall
```

## Kill tests

`integration-tests/phase0/helper-kill.sh userns` needs no root (dummy LAN in a
user namespace; cleanup is invoked the way `ExecStopPost` would). `sudo
helper-kill.sh systemd LAN_IF PHONE_IP` runs the same matrix against the real
units. Stages: client SIGKILL after start / mid-traffic / before stop /
disconnect without Stop; helper SIGKILL at `pending`, `applied`, `done` of each
of the six mutations, while `active`, and at `undo_pending`, `undo_applied`,
`undone` of each — 231 checks. The crash hook is a root-owned file
(`/run/routedroid/crash-at`) the helper compares stage names against; it is
deleted the moment it fires, so only one process dies per injected stage and the
`ExecStopPost` cleanup that follows runs unhooked.
