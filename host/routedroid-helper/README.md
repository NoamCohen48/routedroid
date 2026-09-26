# routedroid-helper

The privileged side of Routedroid (architecture §5.3). `routedroidd` is its only
controller.

- **privileged, per-session, socket-activated** systemd service (`Restart=no`):
  the socket unit is `Accept=yes`, every controller connection gets its own
  `routedroid-helper@<n>.service` instance, so several phones run at once
  and one instance's crash never touches another's session;
- **typed requests only** over a Unix `SOCK_SEQPACKET` socket owned by group
  `routedroid` (`Start{lan_if, phone_ip, tun, mtu}`, `Stop`, `Ping`); the helper
  derives the host address/prefix itself and validates every name;
- **operator policy** (`/etc/routedroid/helper.toml`, root-owned, not writable
  by others; `--policy` overrides) names the LAN interfaces and phone address
  blocks a controller may use; anything else, or an unreadable policy, is
  `Refused` before the kernel is touched. Gateways, the host's own addresses,
  known neighbours, already-routed addresses and the network/broadcast
  address are refused too;
- **exclusive TUN ownership** with whole-packet relay to the controller over the
  same seqpacket connection (`[0x10][IPv4 packet]`);
- **write-ahead journal** (`<state>/journal/<session>.journal`, state dir
  `/var/lib/routedroid` or `--state-dir`): `pending` → apply → `done`, and
  `undo_pending` → undo → `undone`, each line fsynced before the next step. The
  journal also reserves the session's TUN name and phone address until it is
  resolved, so an orphan never collides with a new session;
- **ownership tags**: the TUN's `ifalias` and the nft table's comment carry
  `routedroid:<session>`, and routes use protocol 82; undo removes only objects
  that carry the session's own tag, never a same-named stranger;
- **independent cleanup**: `ExecStopPost=routedroid-helper cleanup` replays any
  unresolved journal after *any* exit (including SIGKILL), and every `serve`
  does the same before it accepts; `routedroid-helper check` exits 1 while an
  orphaned or unreadable journal exists;
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
cargo build --release -p routedroid-helper
sudo host/routedroid-helper/install.sh              # units, group, deny-all policy; re-login or use `sg routedroid`
sudoedit /etc/routedroid/helper.toml                # allow your LAN interface and phone addresses
sudo host/routedroid-helper/install.sh --uninstall
```

The test rigs need the crash hook and the stand-in controller, which exist only
with `--features testing` and are never installed:

```sh
cargo build --release -p routedroid-helper --features testing
host/target/release/routedroid-helper-client --lan-if eno1 --phone-ip 10.100.102.222 --bench 2000 --hold 5
```

## Multi-session test

`integration-tests/helper/multi-session.sh` (no root): two sessions on one LAN
interface through one helper (`serve --socket` serves every connection, the
stand-in for `Accept=yes` instances), a third one probing phone-to-phone
traffic, duplicate TUN / address refusals, refcounted sysctl restore, and one
client's SIGKILL leaving the other session intact — 28 checks.

## Kill tests

`integration-tests/helper/kill-matrix.sh userns` needs no root (dummy LAN in a
user namespace; cleanup is invoked the way `ExecStopPost` would). `sudo
kill-matrix.sh systemd LAN_IF PHONE_IP` runs the same matrix against the real
units; the installed policy must allow `LAN_IF` and `PHONE_IP`. Stages: client SIGKILL after start / mid-traffic / before stop /
disconnect without Stop; helper SIGKILL at `pending`, `applied`, `done` of each
of the six mutations, while `active`, and at `undo_pending`, `undo_applied`,
`undone` of each — 231 checks. The systemd mode needs a helper built with
`--features testing` installed. The crash hook is a root-owned file
(`/run/routedroid/crash-at`) the helper compares stage names against; it is
deleted the moment it fires, so only one process dies per injected stage and the
`ExecStopPost` cleanup that follows runs unhooked.
