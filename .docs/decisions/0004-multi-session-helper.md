# 0004 — Multi-session helper

Status: accepted (Phase 2, step 1). Supersedes the "known limit" of [0003](0003-daemon-split.md).

## Problem

The daemon keys sessions by serial and can drive several at once, but the helper served one
session per activation and hard-coded one nftables table, and its sysctl undo restored the
LAN interface's `proxy_arp`/`forwarding` to the value it saw at start — wrong as soon as two
sessions share the interface (the first to stop would switch proxy ARP off under the second).

## Decision

1. **One helper instance per connection.** The socket unit is `Accept=yes`; systemd starts a
   `routedroid-phase0-helper@<n>.service` per controller connection and hands it the accepted
   `SOCK_SEQPACKET` fd. Each instance still runs `check` before and `cleanup` after, and both
   only act on *orphaned* journals (a live session's journal is `flock`ed by its process), so
   instances never touch each other's state. Without systemd (`serve --socket`, the rigs) one
   process serves every connection in its own task; `--once` restores the old one-shot shape.
2. **One nftables table per session**, `inet routedroid_<tun>`. Rules mention only that
   session's TUN and phone address; the forward chain's final drops keep phone-to-phone
   traffic through the host denied even with several TUNs.
3. **Reference-counted sysctl claims.** `/var/lib/routedroid/sysctl/<key>.json` records the
   baseline, the value Routedroid wrote and the holding sessions; every change is under an
   exclusive `flock` on `<dir>/lock` and written atomically. `acquire` adds a holder (and
   writes the value only if the kernel differs); `release` removes one, and the last one out
   restores the baseline only if the kernel still shows Routedroid's value. Journal replay
   after a crash goes through the same claims, so a crashed session releases exactly its own
   share. The journal's `prev` field is informational now.
4. **System-wide address check.** The helper refuses a phone address that is already routed
   (`ip route show <ip>/32` non-empty), whichever process owns the route.

## Consequences

- The Phase 0 kill matrix (231 checks) and Phase 1 emulator rig pass unchanged in substance;
  `integration-tests/phase2/helper-multi.sh` adds 28 checks for two concurrent sessions.
- `install.sh` replaces the old non-template unit; an upgrade re-enables the socket.
- Concurrent `cleanup` runs (two instances stopping together) tolerate each other: a journal
  another instance holds is skipped, not failed.
