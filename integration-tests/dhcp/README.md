# DHCP alias lab (`netns-dhcp.sh`)

Integration test (first run for Phase 0 §3.3) for the raw-socket DHCP client
(`host/routedroid-dhcp`) against dnsmasq, entirely inside network namespaces.
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
 │ routedroid-dhcp (packet)  │             │ sniff.py (AF_PACKET+AUXDATA)    │
 └───────────────────────────┘             └─────────────────────────────────┘
```

## Running

```sh
(cd host && cargo build --release)      # host/target/release/routedroid-dhcp
integration-tests/dhcp/netns-dhcp.sh   # unprivileged: re-execs under unshare -Urnm
sudo integration-tests/dhcp/netns-dhcp.sh
```

Needs `dnsmasq`, `ip`, `python3`. Unprivileged mode needs user namespaces
and the `veth` (and, for check 6, `8021q`) modules already loaded; check 6
prints `SKIP` when VLAN netdevices cannot be created without root.
`ROUTEDROID_DHCP=/path/to/binary` overrides the binary. The logs, lease files, sniffer
trace and state files are kept when a check fails, or always with `KEEP_TMP=1`. `ALL … PASSED` and exit 0
only when every check passes.

Root-free unit tests: `(cd host && cargo test -p routedroid-dhcp)` — option
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
  `sudo host/target/release/routedroid-dhcp acquire --iface eno1 --client-id routedroid:phase0:test1 --state /tmp/rd-test1.json --hold 120 --renew-after 30 --release-on-exit`
  and watch the PC's own lease (`nmcli device show eno1`) stay unchanged.
- Whether the server unicasts the RENEW ACK to `ciaddr` (needs our ARP
  reply) or broadcasts; whether the switch/AP drops frames from a MAC/IP pair
  it did not learn via its own DHCP snooping.
- VLAN hardware offload: on a NIC with `rx-vlan-offload`, frames bound on a
  VLAN netdevice still arrive untagged, but `PACKET_AUXDATA` on the *parent*
  is the only place the tag is visible — same as the veth result here.
