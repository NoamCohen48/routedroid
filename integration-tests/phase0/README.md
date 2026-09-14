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
