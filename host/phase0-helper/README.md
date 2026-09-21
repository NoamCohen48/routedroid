# phase0-helper — helper gate spike

Proves the architecture §5.3 shape before Phase 1 builds the real daemon:

- **privileged, per-session, socket-activated** systemd service (`Restart=no`):
  the socket unit is `Accept=yes`, every controller connection gets its own
  `routedroid-phase0-helper@<n>.service` instance, so several phones run at once
  and one instance's crash never touches another's session;
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
- **controller death** closes the socket → the helper undoes everything and exits;
- **shared sysctls are reference-counted** (`/var/lib/routedroid/sysctl/<key>.json`,
  updated under `flock`): the first session on a LAN interface records the
  baseline of `forwarding`/`proxy_arp`, later ones add themselves as holders,
  the last one out restores the baseline — and only if the kernel still shows
  the value Routedroid wrote.

Mutations per session, in order (deny-first): TUN, nftables table
`inet routedroid_<tun>` (one table per session), `forwarding` on the TUN and LAN
interface, `proxy_arp` on the LAN interface, `/32` route to the phone via the
TUN. A phone address that is already routed anywhere on the host is refused.

## Run

```sh
cargo build --release -p phase0-helper
sudo host/phase0-helper/install.sh              # units + group; re-login or use `sg routedroid`
phase0-helper client --lan-if eno1 --phone-ip 10.100.102.222 --bench 2000 --hold 5
sudo host/phase0-helper/install.sh --uninstall
```

## Multi-session test

`integration-tests/phase2/helper-multi.sh` (no root): two sessions on one LAN
interface through one helper (`serve --socket` serves every connection, the
stand-in for `Accept=yes` instances), a third one probing phone-to-phone
traffic, duplicate TUN / address refusals, refcounted sysctl restore, and one
client's SIGKILL leaving the other session intact — 28 checks.

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
