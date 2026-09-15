# Phase 0 §3.1 namespace lab

Throwaway integration test for the minimal packet tunnel
(`host/phase0-tunnel`) against a fake Android client, entirely inside Linux
network namespaces. No phone, no ADB, no changes to the real host network.

```text
 phone-ns                     host-ns                           lan-ns
 ┌──────────────┐   frames    ┌──────────────────────────┐      ┌─────────────┐
 │ phonetun0    │ ═══════════▶│ phone0 (Rust TUN)        │ veth │ hllan       │
 │ 192.168.10.74│  TCP over   │ hlhost 192.168.10.1/24 ──┼──────┼─192.168.10.50│
 │ default→tun  │  127.0.0.1  │ mgmt0  10.99.0.1/24      │      │             │
 └──────────────┘ (host-ns lo)│ nft inet routedroid      │      └─────────────┘
  fake_android.py             └──────────────────────────┘
```

`fake_android.py` (stdlib only) plays the Android side: it creates an
`IFF_TUN|IFF_NO_PI` interface inside phone-ns (the `VpnService` tun), then
`setns()`-es into host-ns and connects to the tunnel's loopback port — the
same hop `adb reverse` provides on real hardware. It runs the handshake,
configures its tun from `CONFIGURE_VPN`, sends `VPN_READY`, and pumps one
IPv4 packet per `IP_PACKET` in both directions with bounded queues.

## Running

```sh
(cd host && cargo build)                 # produces host/target/debug/phase0-tunnel
sudo integration-tests/phase0/netns-tunnel.sh
```

Without root you can run it in an unprivileged user namespace (needs the
`veth`, `dummy`, `tun` and `nf_tables` modules already loaded, which is the
case on most desktops):

```sh
unshare -Urnm --propagation private bash -c \
  'mount -t tmpfs none /run && mkdir -p /run/netns && exec integration-tests/phase0/netns-tunnel.sh'
```

`PHASE0_TUNNEL=/path/to/phase0-tunnel` overrides the binary location. The
script prints `PASS`/`FAIL` per check and exits non-zero on any failure. A
`trap` tears everything down (SIGINT to the tunnel, TERM/KILL to the fake
client, `ip netns del` for the three namespaces) on any exit.

Standalone, root-free checks of the codec and state machine:

```sh
(cd host && cargo test && ./target/debug/phase0-tunnel selftest)
```

## What it sets up in host-ns (mirrors architecture.md §9–§12)

- deny-first `table inet routedroid` **before** any forwarding is enabled:
  - raw prerouting (`-300`): `iifname phone0 ip saddr != ALIAS drop`
  - input (`-10`): alias → `192.168.10.1` accept, then `iifname phone0 drop`
  - forward (`-10`): exact `phone0↔hlhost` / alias / `192.168.10.0/24` pairs,
    then terminal `iifname phone0 drop` and `oifname phone0 drop`
  - postrouting/output (`-10`): only the alias may leave via `phone0`
  - named counters so tests can prove which rule fired
- `net.ipv4.ip_forward` forced to 0; only
  `conf.phone0.forwarding`, `conf.hlhost.forwarding`, `conf.hlhost.proxy_arp`
  are set to 1 (per-interface, the §12 question)
- `192.168.10.74/32 dev phone0 src 192.168.10.1`
- `mgmt0` dummy with `10.99.0.1` as the stand-in for an *unselected* host
  address

## Checks vs. the §3.1 acceptance list

| §3.1 acceptance item | Lab check | Status |
|---|---|---|
| Android can ping the PC and one LAN host through the TUN path | `ICMP phone -> PC`, `ICMP phone -> LAN host` | proven in ns |
| PC and a LAN host can ping the Android VPN address | `ICMP LAN host -> phone`, `ICMP PC -> phone`, proxy-ARP neighbour check | proven in ns |
| LAN host can initiate TCP and UDP traffic to Android | HTTP `curl` LAN→phone and PC→phone, 200 KiB transfer, UDP echo | proven in ns |
| Linux forwards the original phone address without NAT | phone→LAN HTTP; LAN server's access log shows `192.168.10.74` | proven in ns |
| disconnecting ADB does not cause unbounded memory growth | phase B: peer `SIGKILL`ed under a `ping -f` flood; tunnel exits ≤ 5 s with "peer closed", TUN gone. Queues are bounded (256) by construction in both pumps | proven for the host side; real ADB stall behaviour needs hardware |
| spoofed packets from the phone are dropped before routing | `ping -I 192.168.10.99` to LAN host and to PC both fail; `spoof_drop` counter in raw prerouting increases | proven in ns |
| valid-alias traffic reaches selected host address, not unselected ones | alias → `192.168.10.1` works; alias → `10.99.0.1` fails with `input_drop` counter increase | proven in ns |
| TUN reads/writes are one raw IPv4 packet, no PI header | `ip -d link` shows `pi off`; both pumps validate IPv4 `total_length == frame body` on every packet (a PI header would fail this) | proven in ns |
| teardown restores route, nftables, sysctl, reverse-map, link baseline | root-namespace snapshot (routes, links, addrs, nft ruleset, sysctls, netns list) diffed before/after; TUN + `/32` route verified gone inside host-ns | proven for everything except the ADB reverse map (not exercised without adb) |
| per-interface forwarding sufficient? (§12) | `ip_forward=0` in host-ns, forwarding works with only per-interface `forwarding=1` on `phone0` and `hlhost` | proven on this kernel |

Also covered: graceful shutdown (SIGINT → `STOP` → fake client exits 0),
periodic and final counters, session bound to the launch `session` id.

## Still needs real hardware

- `adb reverse` stability, `adb reverse --list` parsing/removal on exit, the
  `am start` bootstrap and `VpnService.protect()` of the control socket.
- Android VPN inbound delivery: whether packets written to the `VpnService`
  fd for the phone's own address reach Android apps (and app-side
  `SO_BINDTODEVICE`/UID routing quirks), and outbound capture of all app
  traffic by the `0.0.0.0/0` route.
- Real Wi-Fi/Ethernet proxy ARP (APs may block station-side proxy ARP) and
  MTU/PMTU behaviour on a physical LAN.
- The supervising helper, journaling, and `SIGKILL`-at-every-boundary gate
  described in plan §3 are Phase 0 work items outside this §3.1 probe.

---

# Phase 0 §3.3 DHCP alias lab (`netns-dhcp.sh`)

Throwaway integration test for the raw-socket DHCP probe
(`host/phase0-dhcp`) against dnsmasq, entirely inside network namespaces.
The probe must obtain, renew, restore and release *additional* leases for
phone identities (option 61) on an interface that already has "the PC's
own" address, and it must never add a lease address to that interface.

```text
 host-ns                                   lan-ns
 ┌───────────────────────────┐    veth     ┌─────────────────────────────────┐
 │ hv     192.168.50.10/24   ├─────────────┤ lv     192.168.50.1/24          │
 │        (static, "the PC") │             │        dnsmasq .100-.150, 1h    │
 │ hv.10  192.168.60.10/24   │  802.1Q 10  │ lv.10  192.168.60.1/24          │
 │        (check 6)          │             │        dnsmasq .100-.150 (VLAN) │
 │ phase0-dhcp (AF_PACKET)   │             │ sniff.py (AF_PACKET+AUXDATA)    │
 └───────────────────────────┘             └─────────────────────────────────┘
```

## Running

```sh
(cd host && cargo build --release)      # host/target/release/phase0-dhcp
integration-tests/phase0/netns-dhcp.sh   # unprivileged: re-execs under unshare -Urnm
sudo integration-tests/phase0/netns-dhcp.sh
```

Needs `dnsmasq`, `ip`, `python3`. Unprivileged mode needs user namespaces
and the `veth` (and, for check 6, `8021q`) modules already loaded; check 6
prints `SKIP` when VLAN netdevices cannot be created without root.
`PHASE0_DHCP=/path/to/binary` overrides the binary; `KEEP_TMP=1` keeps the
logs, lease files, sniffer trace and state files. `ALL … PASSED` and exit 0
only when every check passes.

Root-free unit tests: `(cd host && cargo test -p phase0-dhcp)` — option
parser fuzzing (fixed-seed, never panics), RFC 1071 checksum vectors, a
golden DISCOVER frame (independently verified byte-for-byte), RFC 3442
classless-route decoding, builder/state-table conformance.

## What the probe does

- `AF_PACKET`/`SOCK_RAW` bound to the interface index, classic BPF filter
  (IPv4 + UDP + not-a-fragment + dst port 68, or ARP request) attached with
  `SO_ATTACH_FILTER` *before* `bind()`, `PACKET_AUXDATA` on (VLAN tag and
  checksum-not-ready status are read per frame).
- Ethernet/IPv4/UDP/BOOTP/DHCP built by hand; IPv4 header and UDP
  checksums computed; `chaddr` = interface MAC, option 61 = `00` + client-id
  string, option 55 = `1,3,6,51,54,58,59,121`, option 57 = 1500, BROADCAST
  flag, random XID per transaction, `secs` since start, RFC 2131 backoff
  (4 s doubling to 64 s plus jitter) under a global `--timeout`.
- Replies are accepted only if XID, `op=2`, `htype/hlen`, `chaddr` and the
  magic cookie match and option 53 is present; options are length-checked
  individually; the UDP checksum is verified unless the kernel flags the
  frame `TP_STATUS_CSUMNOTREADY` (locally generated over veth).
- States per architecture.md §6.1: DISCOVER → OFFER(s) → SELECTING REQUEST
  (50 + 54) → ACK; INIT-REBOOT REQUEST (50, no 54, broadcast); RENEW
  (unicast raw frame to the ACK's source MAC, `LEASE_IP -> SERVER_IP`,
  `ciaddr`, no 50/54); REBIND (same, broadcast) at T2; RELEASE (unicast,
  `ciaddr` + 54). SIGINT/SIGTERM during `--hold` takes the same exit path.
- While a lease is known the probe answers ARP requests for the lease address
  with the interface MAC (never a local address). Without that no server can
  deliver the unicast RENEW/REBIND ACK it is required to send to `ciaddr`.
  `--no-arp` turns this off to demonstrate the failure.

## Checks vs. the §3.3 acceptance list

| §3.3 acceptance item | Lab check | Status |
|---|---|---|
| PC retains its original lease/address | `ip -4 addr` on `hv` is exactly `192.168.50.10/24` before, after acquire, after concurrent acquires, after renew; no `.100-.150` address anywhere in host-ns | proven in ns |
| extra identity receives a distinct lease | check 1: lease in range, record fields (prefix/router/server id/server MAC/lease) match dnsmasq; lease file lists the hex client-id | proven in ns |
| several identities | check 2: two concurrent clients on the same socket type get two different leases, XID filtering keeps them apart | proven in ns |
| renewal works | check 3: `renew` from state → ACK, same address; sniffer proves REQUEST unicast to the server MAC from `LEASE_IP` with `ciaddr`, and the ACK unicast back to `LEASE_IP`/host MAC; dnsmasq log has no "broadcast response" | proven in ns |
| INIT-REBOOT restores a persisted lease and handles NAK | check 4: valid state → ACK exit 0 (broadcast REQUEST, `ciaddr` 0); state edited to `.200` → NAK exit 3, real lease untouched | proven in ns (dnsmasq needs `--dhcp-authoritative` to NAK instead of staying silent → exit 4) |
| release works | check 5: unicast RELEASE on the wire, `DHCPRELEASE` logged, address gone from the lease file | proven in ns |
| broadcast replies captured reliably | OFFER/ACK arrive as Ethernet broadcast to `255.255.255.255` (sniffer + client log) although the host has no such lease address | proven in ns |
| unicast raw-frame renewal/release without a local alias | checks 3 and 5 | proven in ns |
| VLAN-netdevice operation and packet metadata | check 6: acquire on `hv.10` gets a `192.168.60.x` lease; the parent-side sniffer sees the DISCOVER with VLAN 10 in `PACKET_AUXDATA`; the client bound to the VLAN device sees untagged frames (kernel strips the tag below the VLAN device) | proven in ns |
| NetworkManager/systemd-networkd not disturbed | not exercised here (no manager in the namespace); the probe never touches addresses, routes or the manager's DHCP socket | needs real host |
| ARP conflict detection, DECLINE, announcements, withdrawal | not part of this probe (the ARP responder only *answers* for the lease) | not exercised |

## Still needs real hardware

- A real router/AP: does it honour option 61 for a second identity on the
  same MAC, or key leases by `chaddr` only (then the phone identity and the
  PC would fight over one lease)? Run
  `sudo host/target/release/phase0-dhcp acquire --iface eno1 --client-id routedroid:phase0:test1 --state /tmp/rd-test1.json --hold 120 --renew-after 30 --release-on-exit`
  and watch the PC's own lease (`nmcli device show eno1`) stay unchanged.
- Whether the server unicasts the RENEW ACK to `ciaddr` (needs our ARP
  reply) or broadcasts; whether the switch/AP drops frames from a MAC/IP pair
  it did not learn via its own DHCP snooping.
- VLAN hardware offload: on a NIC with `rx-vlan-offload`, frames bound on a
  VLAN netdevice still arrive untagged, but `PACKET_AUXDATA` on the *parent*
  is the only place the tag is visible — same as the veth result here.
