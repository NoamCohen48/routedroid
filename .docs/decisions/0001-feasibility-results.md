# 0001: Phase 0 Feasibility Results

Status: in progress (gate 5-wireless and §3.4 still open). Dates: 2026-09-14..16.

Test assets: `host/phase0-tunnel`, `android/` (`dev.routedroid.phase0`),
`integration-tests/phase0/{netns-tunnel.sh,lan-proxyarp.sh,phone-multialias.sh,emulator-userns.md}`.

Devices: Samsung SM-J810G, Android 10 (API 29), USB ADB; AOSP emulator
`sdk_gphone64_x86_64`, Android 14 (API 34). Host: Arch Linux, kernel 6.18,
nftables 1.1.7, Docker + firewalld active, `eno1` 10.100.102.18/24 behind an
ISP home router; second LAN host: laptop on the same router's Wi-Fi.

## Gate table

| # | Gate | Environment | Evidence | Result | Scope decision |
|---|---|---|---|---|---|
| 0 | Static bidirectional routed tunnel, no NAT | netns lab (38 checks); emulator; Samsung USB | captures on `phone0` show unchanged addresses; md5-exact TCP 10 MB each way (1.0–1.6 s over USB); UDP; ICMP; bounded RSS under ADB stall; clean teardown both sides | **PASS** | Architecture stands |
| 4 | Inbound TCP/UDP/ICMP written to `VpnService` reach apps | Android 10 Samsung, Android 14 emulator | LAN/host-initiated TCP into `nc -l` on phone, ICMP echo replies, UDP (emulator; toybox on Android 10 cannot listen UDP) | **PASS** | — |
| 2 | Proxy ARP delivers inbound through real Ethernet/Wi-Fi | PC on Ethernet, laptop on ISP router Wi-Fi, manual alias 10.100.102.222 | laptop `ip neigh` resolves alias to PC MAC; laptop-initiated ICMP and 200 KB TCP to phone; phone reaches router, laptop, Internet, DNS | **PASS** (one AP model) | Wi-Fi client side proven; PC-on-Wi-Fi not yet tested |
| 3 | Android selects matching source among multiple aliases | Android 10 Samsung and Android 14 emulator, `/32`+routes and `/24`+routes | ICMP and TCP toward every destination used the first alias; `ip route show table N` on the phone shows `dev tun0` routes with no `src`; inbound to both aliases works, incl. listener bound to the secondary | **FAIL (platform-wide)** | See below |
| 5 | ADB stable while VPN owns default route | USB (Samsung), emulator | ~25 000 packets, `adb shell`/`install`/`logcat` unaffected | **PASS for USB**; wireless untested (device lacks TLS debugging) | Wireless ADB remains open |
| 1 | Extra DHCP identity gets an independent lease | dnsmasq namespace lab (41 checks); ISP home router on `eno1` (Ethernet) | Router offered/ACKed `10.100.102.15` for client-id `routedroid:phase0:test1` on the PC's MAC while NetworkManager kept `10.100.102.18` (client-id `01:<mac>`); unicast RENEW with `ciaddr` ACKed (router returned remaining lease 86370 s, not a fresh 86400); unicast RELEASE on exit; `eno1` never carried the alias | **PASS** (Ethernet, one router model) | Automatic mode allowed on Ethernet; Wi-Fi-attached PC still to test |
| — | Protected ADB-stdin bootstrap | — | not yet run | **OPEN** | §3.4 |
| §12 | Per-interface forwarding sufficient without global `ip_forward` | netns lab, kernel 6.18 | forwarding worked with `ip_forward=0` and only `conf.{phone0,lan}.forwarding=1` | **PASS** | Keep global forwarding as operator prerequisite only where needed |

## Gate 3: what was observed

Setup (`integration-tests/phase0/phone-multialias.sh`): host side of `phone0`
carries three subnets standing in for an office LAN, a lab LAN and the
Internet (`10.77.0.1`, `10.78.0.1`, `10.99.0.1`); the phone receives two
aliases, `10.77.0.2` (primary, added first) and `10.78.0.2`, plus routes
`10.77.0.0/24`, `10.78.0.0/24`, `0.0.0.0/0`.

Observed on the phone (Samsung, Android 10; identical shape on Android 14):

```text
inet 10.77.0.2/24 scope global tun0
inet 10.78.0.2/24 scope global tun0:1
14000: from all iif lo oif tun0 uidrange 0-99999 lookup 1031
table 1031: default        dev tun0 proto static scope link
table 1031: 10.77.0.0/24   dev tun0 proto static scope link
table 1031: 10.78.0.0/24   dev tun0 proto static scope link
```

No route carries a `src`. Captured on `phone0` while the phone pinged and
connected (unbound sockets, `ping` and toybox `nc`):

```text
10.77.0.2 -> 10.77.0.1   (office: correct)
10.77.0.2 -> 10.78.0.1   (lab: WRONG, expected 10.78.0.2)
10.77.0.2 -> 10.99.0.1   (default: correct, primary)
TCP peers seen by host listeners: 10.77.0.2, 10.77.0.2, 10.77.0.2
```

Same result with `/32` aliases. Inbound direction, both aliases:

```text
host -> 10.77.0.2 ICMP           PASS
host -> 10.78.0.2 ICMP           PASS
TCP -> 10.77.0.2 (0.0.0.0 listener)   PASS
TCP -> 10.78.0.2 (0.0.0.0 listener)   PASS
TCP -> 10.78.0.2 (listener bound to 10.78.0.2)   PASS
```

The problem, stated plainly: a phone with a foot in two LANs can be *called*
on either address, but every connection it *opens* leaves with the office
address. On the lab LAN that packet arrives with a source from a foreign
subnet; the lab host replies via its default gateway, which has no route to
the office alias, and the reply is lost. Only a router route (Model A) or NAT
on the PC could repair it, and the operator controls neither router.

## Gate 3 analysis and decision

Android's `VpnService.Builder.addRoute()` installs routes as
`PREFIX dev tun0 proto static scope link` with no preferred source. For an
on-link route without a gateway the kernel's `__fib_res_prefsrc` calls
`inet_select_addr(dev, nh_gw = 0, scope)`, and with a zero destination hint it
returns the first primary address on the device. The alias prefix therefore
never influences selection; both representations behave identically. There is
no public API to set `RTA_PREFSRC` or to add a per-destination source rule.

What still works with several aliases on one VPN:

- the phone is reachable on every alias; replies use the alias that was
  addressed (socket-local address), so LAN-initiated sessions to a secondary
  alias carry the correct source end to end;
- applications that bind a socket to a specific alias get that source
  (listener-bound case verified; connect-bound is the same kernel path).

What does not work: phone-*initiated* traffic toward a secondary LAN carries
the primary alias, which that LAN cannot return without a router route.

Decision (per the architecture's rule "restrict, never silently NAT"):

1. **Version 1 supports exactly one selected interface and one alias per
   phone** (operator decision, 2026-09-15: routers are not under the
   operator's control, so neither Model A nor an inbound-only secondary mode
   is worth shipping now). Multi-interface support is removed from the
   architecture and plan, not degraded.
2. The `/32` representation is adopted (simplest; behaviour is identical and
   it never implies on-link semantics). The actual LAN prefix is still sent
   for route construction.
3. Model A (one routed phone subnet, router-configured) remains the path to
   full multi-LAN outbound identity, as already designed.
4. Re-test on each new Android major; if a future `VpnService` API allows
   per-route preferred sources, revisit.

## Gate 1 notes

- The ACK for a unicast RENEW is addressed to the alias IP; since the alias is
  never configured on the PC, something must answer ARP for it or the router
  cannot deliver the ACK. The probe answers ARP for the alias while it holds
  the lease; in the product this is the same presence proxy ARP provides.
  Renewal must therefore never be attempted before proxy ARP/route state is
  active, or must use REBIND (broadcast) as fallback.
- The router refreshes by returning the *remaining* lease time; T1/T2 are
  recomputed from the ACK each time, never from the original lease.
- No VLAN tags were seen on `eno1`; PACKET_AUXDATA path verified in the lab.

## Host firewall coexistence (found on the real LAN)

- Docker: `iptables -P FORWARD DROP` in the `ip filter` base chain (priority 0)
  drops everything Routedroid's table accepted at priority -10. Remedy:
  `DOCKER-USER` accept rules scoped to `phoneN`. Safe because Routedroid's
  own drops run first; the rule only removes Docker's veto.
- firewalld: `phoneN` in no zone -> `iif eno1 oif phoneN` rejected with
  admin-prohibited while the reverse direction passed. Remedy: register
  `phoneN` with firewalld (D-Bus, runtime) in the selected interface's zone or
  a dedicated forward-only zone.

Both become Phase 2 helper responsibilities plus `doctor` checks, not manual
steps.

## Bugs found by hardware that the emulator or lab could not show

1. Android pumps deadlocked: `SocketChannel` adaptor streams share
   `blockingLock()`; reader starved writer. Fixed with direct channel I/O.
2. `InetAddress.getLoopbackAddress()` is `::1` on Android 10 (Samsung), adbd's
   reverse listener is IPv4-only. Fixed with explicit `127.0.0.1`.
3. `lan-proxyarp.sh` cleanup ran twice (INT then EXIT). Fixed.

## Still required before Phase 1 is declared

- Gate 1 on a Wi-Fi-attached PC (Ethernet done).
- §3.4 bootstrap provider on Android 10 and 14.
- Wireless ADB (Android 11+ TLS) under the VPN default route.
- Supervising helper with journal and `SIGKILL` boundaries (§3.1 helper gate).
