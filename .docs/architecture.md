# Routedroid Architecture

## 1. Purpose

Routedroid makes an unrooted Android device a reachable IPv4 host on one or more networks connected to a Linux PC. It transports raw IP packets over ADB and uses Linux routing rather than replacing Android connections with host sockets.

The primary deployment assumes:

- the operator controls the Linux PC but not the attached LAN routers;
- the phone may use Android `VpnService` and may visibly report an active VPN;
- the selected LAN may have DHCP but cannot be given a static route to a separate phone subnet; and
- the desired result is normal bidirectional IP connectivity, not transparent Ethernet bridging.

The primary mode therefore gives the phone one DHCP-assigned, local-looking IPv4 address from **one operator-selected host interface** and uses proxy ARP to make that address reachable.

Version 1 supports exactly one selected interface per phone. Phase 0 showed (decision record 0001, gate 3) that Android installs VPN routes without a preferred source, so a phone with several aliases always initiates traffic from the first one; multi-LAN outbound identity is therefore impossible without router routes or NAT, and the operator does not control routers. Multi-interface support is removed from version 1 rather than shipped half-working.

## 2. Scope

### Version 1

- Linux host only.
- One or more Android devices connected through authorized ADB.
- Android 8.0 or newer, subject to validation in the compatibility matrix.
- IPv4 packet transport.
- TCP, UDP, ICMP, and other IPv4 payload protocols.
- One operator-selected host LAN per phone.
- Automatic DHCP lease acquisition for the phone alias.
- Manual address fallback.
- Proxy ARP and host routing.
- Explicit selection of the single permitted host interface, which also carries default Internet traffic and DNS.
- Clean restoration after normal shutdown and deny-safe cleanup by a supervising privileged helper if the controller crashes.

### Later Versions

- IPv6 addressing and NDP proxying or routed-prefix support.
- Router-configured dedicated phone subnets (Model A), which is also the only sound path to multi-LAN outbound identity.
- Multiple selected interfaces with inbound-only reachability on secondary LANs, if there is demand.
- Public-address and VPS/WireGuard integrations.
- Non-Linux hosts.
- Optional discovery relays such as mDNS and SSDP.

### Explicit Non-Goals

- Hiding Android's VPN status.
- Ethernet TAP or Layer-2 bridging through `VpnService`.
- Guaranteeing operation on networks that reject extra DHCP identities or proxy ARP.
- Automatically joining every host interface without operator approval.
- Bypassing network admission controls, client isolation, or enterprise policy.
- Replacing Android's normal Wi-Fi or cellular configuration outside the active VPN.

## 3. Network Model

Example with one selected host interface (the second LAN below is shown only to make clear that it is *not* selected and is unreachable from the phone):

```text
Office LAN                        Lab LAN
192.168.10.0/24                   172.16.20.0/24
       |                                 |
eth0: 192.168.10.20              eth1: 172.16.20.20
       +---------------+-----------------+
                       |
                 Linux routing
                       |
                    phone0
                       |
                 packet framing
                       |
                      ADB
                       |
               Android VpnService
                       |
              Android applications

Phone alias:
  192.168.10.74/32, obtained through DHCP on eth0 (selected)
  eth1 / 172.16.20.0/24 is not selected: no alias, no forwarding
```

Linux does not own the phone alias as a local host address. It installs one host route:

```text
192.168.10.74/32  dev phone0  src 192.168.10.20
```

Proxy ARP on `eth0` makes the PC answer neighbor requests for the phone alias. Packets keep the phone address while Linux forwards them between the physical interface and `phone0`.

## 4. Components

### 4.1 Android Application

Use Kotlin and the standard Android SDK.

Primary components:

- `MainActivity`: VPN authorization, device status, and diagnostics.
- `RoutedroidVpnService`: foreground service that owns the VPN file descriptor.
- `HostTransport`: protected loopback TCP connection carried by `adb reverse`.
- `FrameCodec`: bounded framing for control messages and raw packets.
- `VpnConfigurator`: validates host configuration and establishes the VPN.
- `PacketPump`: copies packets between the VPN file descriptor and transport with bounded queues.
- `StatusStore`: exposes current session, aliases, DNS, routes, and errors to the UI.

The application does not implement TCP, UDP, NAT, DHCP, or ARP. Android's network stack continues to own transport protocols. The application only carries complete IP packets and session control messages.

### 4.2 Host Controller

Use Rust for the Linux host executables. Rust provides memory-safe packet parsing, good async and system-call support, and produces single deployable binaries.

The controller is a headless per-user daemon, `routedroidd`, that owns every *device connection* and applies all policy; user interfaces (`routedroid` CLI, `routedroid-tui`, later a tray) are separate processes that drive it over a user-owned Unix socket with JSON lines (`routedroid-ipc`; decision 0003). The daemon is unprivileged; the privileged helper (§4.3) stays a separate system service.

Two words, deliberately distinct (decision 0005):

- a **device connection** is one phone being reachable on the LAN — the daemon owns it, it outlives the client that asked for it, and every client may watch, start or stop it;
- a **client connection** is one CLI or TUI process attached to the control socket; it owns nothing.

"Session" now means only what the wire protocols call a session: the authenticated conversation with the app (§7.3), and the helper's journaled bundle of privileged mutations (§4.3).

Inside the daemon there is no object that owns everything (decision 0006). The process is built from three components, each responsible for one thing and each holding only what that thing needs: `AttachedDevices` (what adb reports, kept current by its own poll loop), `DeviceConnections` (the phones we have put on the LAN, and everything needed to start one), and `EventBus`. A client connection is handed those three and has no handle to anything above them, so it cannot reach the daemon's own operations — shutdown and lifecycle — at all. The wire's types (`Request`, `Response`, `DeviceInfo`) exist only in the client connection, which joins the device list with the connection states to build the view it sends.

Primary daemon modules:

- `device`: ADB device discovery, package installation checks, launch, and teardown.
- `connection`: per-device connection state machine and lifecycle coordination (the protocol session it drives lives in `session`).
- `transport`: loopback listener, ADB reverse mapping, authentication, and frames.
- `tun`: requests helper-owned TUN sessions and exchanges whole packets over bounded `SOCK_SEQPACKET`.
- `interfaces`: eligible interface discovery and operator selection.
- `dhcp`: additional lease acquisition and renewal per phone/interface identity.
- `neighbor`: ARP conflict detection and proxy ARP configuration.
- `route`: netlink route and policy management.
- `firewall`: narrowly scoped nftables forwarding rules.
- `sysctl`: reference-counted forwarding and proxy ARP settings.
- `state`: durable lease and cleanup metadata.
- `diagnostics`: structured logs and a redacted support bundle.

Initial commands (each a request to the daemon):

```text
routedroid devices
routedroid interfaces
routedroid start --serial SERIAL --lan-if eth0 ...   # attached until Ctrl-C, or --detach
routedroid status
routedroid stop --serial SERIAL
routedroid events                                    # follow the daemon's event stream
routedroid doctor
```

The default is no selected physical interface. This avoids unintentionally forwarding a phone into management, container, VPN, or sensitive networks.

### 4.3 Privileged Network Operations

Phase 0 may use a capability-bearing test process only inside isolated network namespaces or a disposable lab segment. Before any ordinary LAN test, privileged operations and cleanup are owned by a small supervising helper with a narrow request API. The helper exclusively owns the TUN file descriptor and relays whole packets to the controller through a bounded Unix `SOCK_SEQPACKET` channel; the controller never receives a duplicate TUN descriptor.

Each helper session runs as a systemd service with `Restart=no`, an independent root-owned `ExecStopPost` cleanup program, and an `ExecStartPre` reconciliation guard. Every external mutation uses a write-ahead journal: persist and `fsync` intent before the mutation, apply it, then persist and `fsync` completion. Cleanup inspects exact owned kernel resources for both pending and completed entries, so death between mutation and completion recording is recoverable.

If the helper exits for any reason, systemd runs cleanup. Cleanup failure leaves the unit failed. A later explicit start first runs `ExecStartPre`, which refuses startup while an unresolved entry, reserved session, or reserved TUN name remains. Controller loss closes the `SOCK_SEQPACKET` channel and makes the helper stop normally. This independent mechanism, rather than code in a crashing process, removes active nftables state, routes, policy rules, and sysctl references before reuse.

Required Linux capabilities are expected to include:

- `CAP_NET_ADMIN` for TUN, routes, nftables, and relevant sysctls;
- `CAP_NET_RAW` for DHCP and ARP frames; and
- permission to open `/dev/net/tun` and execute ADB for the current user.

The privileged helper accepts validated operations, not arbitrary shell commands. The controller never constructs privileged command strings from phone-provided values.

## 5. Session Lifecycle

Each device follows this state machine:

```text
Disconnected
  -> PreparingHost
  -> AcquiringLeases
  -> AwaitingVpnPermission
  -> ConfiguringVpn
  -> Active
  -> Reconnecting
  -> Stopping
  -> Disconnected

Any state -> Failed -> Stopping -> Disconnected
```

Startup sequence:

1. Validate the ADB device, host privileges, and the selected interface.
2. Allocate a unique `phoneN` TUN interface and bring it up with a conservative MTU.
3. Acquire or restore one valid DHCP lease on the selected interface.
4. Probe the offered address for an existing ARP owner and decline a conflict.
5. Install session-scoped prerouting, forwarding, input, and postrouting rules in a not-ready deny state.
6. Install host `/32` routes toward `phoneN` without adding aliases locally.
7. Enable per-interface proxy ARP and the minimum required forwarding sysctls.
8. Bind a random host loopback port and create a per-device `adb reverse` mapping.
9. Stream the one-time session secret to the protected provider with `adb shell content write` and verify successful one-time storage.
10. Launch the Android bootstrap activity with only non-secret session and device-port references.
11. Mutually authenticate and negotiate the transport protocol before VPN consent or service start.
12. Send the complete VPN configuration to Android.
13. Android obtains VPN consent if necessary, establishes the VPN, and reports readiness.
14. Send ARP announcements for the alias and atomically replace the not-ready rules with active forwarding rules.

Shutdown reverses session-owned operations. Lease release is configurable because retaining a still-valid lease across a short reconnect is safer and faster than repeatedly acquiring new addresses.

## 6. DHCP Alias Acquisition

### 6.1 Behavior

Routedroid behaves as an additional DHCP client on the selected physical interface. It does not start a DHCP server and does not modify the PC's existing DHCP lease.

Each identity uses:

```text
client-id = routedroid:<stable-device-id>:<interface-stable-id>
```

The stable device identifier is derived from the authorized ADB serial through a one-way hash before transmission on the LAN. Raw ADB serials are not disclosed in DHCP packets.

The DHCP implementation uses its own transaction IDs and a raw `AF_PACKET` socket bound by interface index with a narrow classic BPF or eBPF filter. It constructs and validates Ethernet, IPv4, UDP, BOOTP, and DHCP fields and checksums without assigning the lease to the PC. It follows INIT, SELECTING, REQUESTING, INIT-REBOOT, BOUND, RENEWING, REBINDING, expiration, decline, NAK, and optional release behavior.

Required packet behavior is:

| Operation | Ethernet destination | IPv4 source -> destination | BOOTP `ciaddr` | Required DHCP details |
|---|---|---|---|---|
| DISCOVER | Broadcast | `0.0.0.0 -> 255.255.255.255` | Zero | Unique XID, broadcast flag, option 61 |
| SELECTING REQUEST | Broadcast | `0.0.0.0 -> 255.255.255.255` | Zero | Requested address and selected server identifier |
| INIT-REBOOT REQUEST | Broadcast | `0.0.0.0 -> 255.255.255.255` | Zero | Persisted requested address, no selected server identifier |
| RENEW | Resolved server or next-hop MAC | `LEASE_IP -> SERVER_IP` | Lease IP | Unicast raw frame; omit requested-address and server-identifier options |
| REBIND | Broadcast | `LEASE_IP -> 255.255.255.255` | Lease IP | Broadcast to any server; omit requested-address and server-identifier options |
| DECLINE | Broadcast | `0.0.0.0 -> 255.255.255.255` | Zero | Declined address and server identifier |
| RELEASE | Resolved server or next-hop MAC | `LEASE_IP -> SERVER_IP` | Lease IP | Server identifier, best effort |

DHCP uses UDP client port 68 and server port 67. The implementation resolves an on-link server MAC or the appropriate gateway MAC for unicast raw frames, captures ACK and NAK frames before the host IP stack discards packets addressed to an unconfigured lease, and handles broadcast replies. It verifies the XID, client identity, hardware address fields, server selection, and interface before accepting a response.

VLAN interfaces are treated as L3 interfaces in their own right. Packet sockets bind to the selected VLAN netdevice, and tests verify VLAN offload metadata through `PACKET_AUXDATA`; version 1 does not synthesize arbitrary VLAN membership on a physical parent.

The implementation records:

- assigned address;
- subnet mask;
- router and classless static routes when supplied;
- DNS servers;
- DHCP server identifier;
- lease start, T1, T2, and expiration;
- interface index and stable identity; and
- the client identifier used to acquire the lease.

DHCP-provided router and classless-route options are inputs to validation, not commands applied directly. Routedroid accepts only routes whose next hops are reachable through the selected interface and whose destinations do not overlap protected host or phone control ranges. Android receives validated destination prefixes; it does not receive DHCP next-hop semantics that would imply direct Ethernet access.

### 6.2 Ethernet Identity

The initial implementation transmits using the physical interface's source MAC and uses a distinct DHCP option 61 client identifier. The BOOTP `chaddr` and broadcast behavior must be validated against common DHCP servers.

Some networks identify clients only by MAC address or enforce DHCP snooping and dynamic ARP inspection. Failure to obtain a safe independent lease is a capability failure for that interface, not a reason to silently choose an address.

Diagnostics distinguish common failure classes:

- no DHCP offer for the extra client identity suggests a lease/client limit or filtering;
- an ACK followed by switch drops suggests DHCP-snooping or IP/MAC binding;
- successful outbound traffic with failed inbound ARP suggests proxy-ARP suppression or Wi-Fi client isolation;
- failure only between wireless stations suggests AP client isolation;
- missing broadcast or multicast frames while unicast works suggests AP suppression rather than tunnel failure.

Coexistence tests verify that NetworkManager and systemd-networkd do not adopt the alias or alter their address, route, DNS, DHCP state, connectivity result, or interface-management state. Tests include manager reload and restart while a Routedroid lease is active.

### 6.3 Conflict Detection

Before activating a lease, the host performs IPv4 Address Conflict Detection using ARP probes. A conflict causes DHCPDECLINE where appropriate and restarts acquisition with bounded backoff. After the route and deny-safe firewall state exist, the host sends RFC 5227-style ARP announcements using the physical interface MAC and leased IP before enabling forwarding. ARP monitoring continues while active; a confirmed conflict immediately disables forwarding, withdraws proxy behavior and the route, reports the event, and declines or abandons the lease before reacquisition.

ARP probing does not make manually selected addresses safe against future DHCP allocation. Manual mode therefore warns unless the operator confirms that the address is reserved or outside the DHCP pool.

### 6.4 Lease Changes

Public `VpnService.Builder` APIs establish a configured VPN interface; they do not provide a portable way to mutate every property of an active VPN across supported Android versions. If a lease changes address, Routedroid sends a replacement configuration and Android re-establishes the VPN interface. Existing connections may be interrupted.

Normal DHCP renewal should retain the same address and avoid this operation. The host begins renewal early enough to prevent expiration during temporary ADB disruption.

## 7. Android VPN Configuration

The host sends a complete, validated configuration:

- the alias as `/32` (adopted by decision record 0001: Android's source selection ignores the prefix, and `/32` never implies on-link semantics);
- the selected LAN prefix as a route;
- a default route; and
- DNS servers selected by policy.

```text
Address:  192.168.10.74/32
Routes:   192.168.10.0/24, 0.0.0.0/0
DNS:      192.168.10.1
```

A single address makes source selection trivial. Phase 0 recorded that `VpnService.Builder.addRoute()` installs routes without a preferred source, so a second alias would never be used for phone-initiated traffic; version 1 therefore never configures more than one IPv4 address.

The VPN transport socket is protected with `VpnService.protect()` before the default route becomes active. The service does not call `allowBypass()`. Per-application VPN exclusions are outside version 1.

## 8. Packet Transport Protocol

### 8.1 ADB Channel and Bootstrap

For every phone, the host binds an unpredictable loopback-only TCP port, chooses an unused high device port after inspecting that serial's reverse mappings, and installs:

```text
adb -s SERIAL reverse tcp:DEVICE_PORT tcp:HOST_PORT
```

Android connects to `127.0.0.1:DEVICE_PORT`. ADB carries the stream over USB or the already-authorized ADB transport. The host listener is never bound to a LAN address.

The session secret is delivered first over standard input to an exported `BootstrapProvider` using `adb shell content write`. The provider is protected by `android.permission.DUMP`, checks that the Binder caller UID is Android's shell UID, accepts a small fixed-size/versioned bootstrap record through `openFile`, stores it only in private volatile memory, and expires it quickly. Phase 0 validates this shell permission and streaming mechanism on every supported Android version and vendor family. The secret never appears in `adb`, shell, or activity command-line arguments.

The host then launches an explicitly named exported `BootstrapActivity` using `adb shell am start`, passing only non-secret session and port references. The activity is not browsable. It retrieves the pending private bootstrap record, authenticates the host before showing VPN consent or starting a service, and only then starts the non-exported foreground VPN service in compliance with Android background-start rules. Calls without a matching shell-delivered record fail without a VPN prompt, service start, or persistent mutation. Repeated attempts are rate-limited and tested from a hostile Android application.

Every reverse mapping is recorded with its ADB serial, device port, host port, and session. Cleanup first compares `adb -s SERIAL reverse --list` with the recorded mapping and then uses `adb -s SERIAL reverse --remove tcp:DEVICE_PORT`; it never removes a mapping that no longer matches its ownership record. Port collisions retry with a new port.

Version 1 supports USB ADB only. Android's TLS-based wireless debugging is a planned addition once its behaviour under the VPN default route has been verified on hardware (feasibility gate 5-wireless was deferred; see decision 0001); until then `routedroid start` refuses a network serial with an explicit message (`--allow-network-adb` overrides it for that verification; the adb binding itself is transport-agnostic). Legacy unauthenticated or unencrypted network ADB is rejected outright because the packet stream itself is not encrypted by this protocol.

### 8.2 Authentication

The host creates a cryptographically random, single-use 256-bit secret and streams it to the protected Android bootstrap provider through ADB standard input. The secret is never placed in command arguments or sent as an authentication value on the transport.

Mutual authentication uses independent client and host nonces and HMAC-SHA-256:

1. Android sends the protocol version, session identifier, device port, role label, and client nonce.
2. The host replies with a host nonce and `HMAC(secret, "host" || complete transcript)`.
3. Android verifies the host proof and replies with `HMAC(secret, "android" || complete transcript)`.
4. The host verifies the Android proof, both endpoints atomically consume and clear the session secret, and the host allows configuration messages.

The transcript includes both nonces, both role labels, the negotiated version, session identifier, and reverse-port identity to prevent reflection and cross-session replay. Concurrent connection attempts are serialized; only the first successful transcript consumes the secret. Authentication has a short deadline. Secrets and derived values are redacted from logs, removed from persisted state, and zeroized from mutable memory on completion or failure where the language/runtime permits.

ADB authorization remains the primary trust boundary. Mutual proof prevents an unrelated local process from impersonating either endpoint through the loopback listener or a stale reverse mapping.

### 8.3 Framing

Version 1 uses a fixed binary frame header followed by a bounded body:

```text
u32 body_length, network byte order
u8  protocol_version
u8  message_type
u16 flags/reserved
body_length bytes
```

Message types include:

- `HELLO` and `HELLO_ACK`;
- `CONFIGURE_VPN`, `VPN_READY`, and `VPN_ERROR`;
- `IP_PACKET`;
- `PING` and `PONG`;
- `LEASE_UPDATE`;
- `STOP`; and
- `ERROR`.

`body_length` excludes the eight-byte header. Version 1 limits control bodies to 64 KiB and `IP_PACKET` bodies to the negotiated MTU, with an absolute IPv4 ceiling of 65,535 bytes. Legal flags and valid messages are defined per protocol state. Before mutual authentication, only handshake messages are accepted. Duplicate terminal handshake messages, trailing bytes inside fixed-schema controls, unknown required flags, and out-of-state messages close the session.

Control bodies use a documented JSON schema in version 1 because they are infrequent and easy to inspect across Rust and Kotlin. `IP_PACKET` bodies contain exactly one raw IPv4 packet and are never encoded as JSON.

Receivers reject unsupported versions, unknown required flags, zero-length packet frames, bodies over the configured maximum, and packets whose IPv4 total length disagrees with the frame. Control message size and packet MTU have separate limits.

### 8.4 Backpressure

ADB is a reliable ordered stream, so one lost or delayed transfer can block later packets. Each direction uses bounded queues and suspends reads when the next stage is full. It must not accumulate unbounded packets.

Version 1 starts with one stream for simplicity. Performance tests determine whether control and packet traffic need separate ADB reverse streams. Queue depth, drops, packet rate, throughput, and round-trip latency are exported as diagnostics.

### 8.5 Exact TUN Semantics

Linux creates a non-persistent interface with `IFF_TUN | IFF_NO_PI`. Therefore, neither endpoint adds a Linux packet-information header to `IP_PACKET` bodies.

Packet direction is exact:

```text
Android VPN read -> IP_PACKET -> host TUN write -> packet received by Linux
Linux TUN read   -> IP_PACKET -> Android VPN write -> packet received by Android
```

Every successful frame carries one complete packet. A short Android or Linux TUN write terminates and resets the packet path; its unwritten suffix is never submitted as another TUN write. An interrupted TUN operation may be retried only when it transferred zero bytes. TCP transport writes, in contrast, continue until the complete frame header and body are sent. Captures on `phoneN`, Android's VPN boundary, and the physical interface verify direction and unchanged addresses.

## 9. Linux Routing

For every phone alias, Routedroid installs a host route through that device's TUN:

```text
PHONE_IP/32 dev phoneN src HOST_INTERFACE_IP
```

The preferred source ensures that connections initiated by the PC toward a phone alias use the PC address from the corresponding LAN.

Packets arriving from `phoneN` are accepted only when their source address is one of that session's active aliases. This prevents the Android side from spoofing arbitrary LAN addresses.

Forwarding rules bind the alias to the selected physical interface:

```text
192.168.10.74 may enter or leave through eth0 only
```

Phone traffic never consults the host's main routing table. Every session installs a source-policy rule, `from ALIAS/32 lookup TABLE` at priority 1082 (behind `local`, ahead of `main` and of the rules VPN clients add), where TABLE is the alias as a 32-bit number, so the journal's address reservation makes it the session's alone. The table holds only the selected interface's connected route, its gateway (the lease's router, else that interface's own default route; with neither the phone reaches the LAN only), and an `unreachable` default at the highest metric, so the phone's traffic leaves through the selected interface or not at all, also after that interface's routes vanish with it. Rule and routes carry route protocol 82; the plan refuses a table or rule someone else already uses, and undo removes exactly protocol-82 entries of that table. Losing the selected interface ends the session (the DHCP/ARP socket bound to it fails), and undo then skips the RELEASE it can no longer send. The rigs verify the result with `ip route get DEST from ALIAS iif TUN`.

## 10. Proxy ARP

Proxy ARP is enabled only on selected IPv4 interfaces while at least one active phone alias uses that interface. Linux answers ARP for an alias because a more specific `/32` route points through `phoneN`.

Routedroid does not add the alias to the physical interface. This prevents Linux from treating the phone address as local and prevents PC services bound to wildcard addresses from accepting the phone's connections.

The controller records the original per-interface proxy ARP setting and reference-counts use across devices. It restores a setting only when the last Routedroid user exits and only if the current value still matches the value Routedroid installed.

## 11. Firewall and Isolation

Routedroid creates a dedicated `inet routedroid` nftables table with generated, testable session sets. Every base chain has policy `accept` so unrelated host traffic is not governed by Routedroid. Version 1 uses these base-chain roles and documented numeric priorities relative to standard nftables priorities:

- prerouting filter at raw priority (`-300`) drops every packet entering `phoneN` whose source is not in that session's active alias set, before conntrack and route classification;
- input filter at priority `-10` drops all `phoneN` traffic while not ready and allows host-local traffic only when the source is the exact active alias and the destination is in that session's permitted host-address set;
- forward filter at priority `-10` enforces exact TUN, physical-interface, source-alias, and destination-alias pairs;
- postrouting filter at priority `-10` drops packets leaving `phoneN` unless their destination is an active alias owned by that TUN; and
- output validation at priority `-10` allows host-originated traffic into `phoneN` only for an active destination alias and then drops every other `oif phoneN` packet.

Forward rules are directional:

- phone to network: `iif phoneN`, source equals the alias owned on the selected `oif`, and destination belongs to that interface's permitted destination set;
- network to phone: `iif` equals the alias's owning interface, destination equals that alias, `oif phoneN`, and source belongs to that interface's permitted source set;
- the selected interface uses `0.0.0.0/0` permitted sets so Internet requests and replies can traverse the upstream router; and
- terminal `iif phoneN drop` and `oif phoneN drop` rules discard every unmatched path.

Not-ready rules drop all traffic entering or leaving `phoneN`. Established and related accepts repeat the same directional interfaces, alias, and permitted-prefix constraints and cannot precede spoof checks. No `reject` action is used in prerouting or postrouting.

The permitted host-address set contains only addresses on physical interfaces selected for that session plus any explicitly configured host service address. It excludes loopback, management, container, unrelated VPN, and every unselected interface by default. The input chain ends with `iif phoneN drop`, so a valid phone alias cannot reach an unselected host-local address merely because Linux owns it. Changes to host addresses update this set atomically before traffic is permitted.

An accept in Routedroid's table cannot override a later drop in an existing host firewall. Established and related rules are constrained by the same TUN, physical interface, and alias direction and appear only after anti-spoofing rules. Generated ruleset fixtures specify exact hooks, priorities, sets, and ordering.

Required behavior:

- permit established and related return traffic for allowed session paths;
- permit an active alias from `phoneN` only toward its selected interface;
- permit traffic to an active alias only from its selected interface;
- drop packets with source addresses not assigned to the session;
- prevent phone-to-phone forwarding by default;
- prevent forwarding into unselected host interfaces;
- prevent valid-alias traffic from reaching host-local addresses on unselected interfaces;
- remove session rules atomically during teardown.
- validate spoofing attempts aimed at both forwarded destinations and host-local services.

Existing host firewalls may still reject forwarding in later hooks. `routedroid doctor` detects common nftables, firewalld, and UFW conflicts and reports exact remediation rather than disabling an existing firewall.

Forwarding defaults to denied until the VPN is authenticated, configured, and ready.

## 12. Sysctl Management

Routedroid first attempts to enable forwarding only on `phoneN` and selected physical interfaces:

```text
net.ipv4.conf.phoneN.forwarding=1
net.ipv4.conf.<interface>.forwarding=1
net.ipv4.conf.<interface>.proxy_arp=1
```

Phase 0 verifies whether per-interface forwarding is sufficient on supported kernels. If a supported environment requires global `net.ipv4.ip_forward=1`, global forwarding becomes an explicit operator-managed prerequisite rather than a value toggled casually by Routedroid. Deny-safe nftables rules are installed before any forwarding setting is enabled, so unrelated paths do not become permitted.

Strict reverse-path filtering is evaluated per interface. Linux effective behavior may combine `conf/all/rp_filter` with per-interface settings, so doctor records `all`, `default`, `phoneN`, and every selected physical value. It is changed only when an integration test proves that valid policy-routed traffic is being dropped. Loose mode is preferred on only the required interfaces; a global change is an explicit operator prerequisite.

The host state manager stores baseline values and active references. It never writes broad settings such as `conf.all.proxy_arp`. Crash recovery reconciles only resources marked with Routedroid-owned names or metadata.

## 13. Multiple Devices

Each phone receives:

- a unique TUN interface such as `phone0` or `phone1`;
- a unique DHCP client identity on the selected interface;
- distinct DHCP leases;
- a unique ADB reverse port and session secret; and
- isolated route and firewall entries.

Addresses must not be shared. Packet dispatch is based on the TUN interface, not inspection of transport ports.

## 14. DNS

Version 1 uses DNS servers learned on the selected interface unless explicitly overridden. Android sends DNS packets through the VPN like other traffic.

## 15. MTU and Packet Correctness

The initial VPN and TUN MTU is negotiated to one identical value on both endpoints, conservatively 1400 bytes by default. Host routes are checked for a compatible MTU. ADB framing adds no network-path IP header, but a smaller MTU limits queue pressure and avoids assumptions about downstream links. Oversized frames are rejected, and tests cover IPv4 DF packets plus delivery of ICMP fragmentation-needed errors.

The tunnel preserves packet bytes. It does not recalculate TCP or UDP checksums because it does not rewrite addresses. It validates IPv4 version, header length, total length, and frame bounds before injection. Fragmented IPv4 packets are transported unchanged.

ICMP errors needed for Path MTU Discovery must be forwarded. The firewall must not blanket-drop ICMP.

## 16. Failure Handling

- ADB loss makes the supervising helper immediately return the session firewall to not-ready deny state, pauses forwarding, bounds queued packets, and enters `Reconnecting`.
- A short reconnect reuses valid leases and reconstructs the authenticated transport.
- Lease expiration removes the alias and route before another host can receive that address.
- Selected-interface loss disables forwarding and reports it; Routedroid never moves the phone to another interface on its own because that would change the phone's address.
- VPN revocation tears down forwarding and reports a user-actionable error.
- Controller crashes close the bounded packet/control channel, causing the helper to stop and systemd to run the independent cleanup program. If the helper is killed, its exclusively owned non-persistent TUN and dependent routes disappear immediately; `ExecStopPost` reconciles pending and completed journal entries. The unit uses `Restart=no`, cleanup failure leaves it failed, and `ExecStartPre` blocks a later explicit start until reconciliation succeeds. A session TUN name cannot be reused while any unresolved ownership record remains. Tests kill the controller and helper independently while the other process remains alive and at every write-ahead journal boundary.

## 17. Persistent State

State is stored under a root-owned runtime/state directory with restrictive permissions. It contains no packet payloads and no reusable authentication secret. Every network mutation has an intent record durably written before execution and a completion record written afterward. Reconciliation checks kernel state for both kinds of entry rather than assuming that a pending operation did not happen.

Persisted data includes:

- versioned schema number;
- device identity hash;
- the selected interface;
- active or retained DHCP lease metadata;
- TUN and firewall resource names;
- baseline sysctl values; and
- timestamps needed for cleanup and renewal.

Writes use a temporary file, `fsync`, and atomic rename. Invalid or newer schemas fail closed and trigger diagnostic guidance rather than guessed cleanup.

## 18. Observability

Structured logs include session and interface identifiers but redact session secrets and raw ADB serials. Counters include:

- packets and bytes in each direction;
- malformed and dropped frames;
- queue occupancy and backpressure events;
- DHCP state transitions and renewal deadlines;
- ARP conflicts;
- route and firewall failures;
- reconnect count; and
- current VPN configuration version.

Packet capture is an explicit diagnostic mode and is off by default.

## 19. Security Boundaries

- Only ADB-authorized devices may be started.
- The phone configuration is treated as untrusted input on the host.
- The host is authoritative for aliases, routes, DNS, and allowed interfaces.
- Frame lengths and IP headers are validated before allocation or injection.
- The host rejects spoofed source addresses from the phone.
- The Android app accepts configuration only after authenticating the host through the session-secret handshake.
- ADB listeners bind only to loopback.
- Privileged helper requests are typed and validated.
- Forwarding is restricted to explicitly selected networks.

## 20. Key Feasibility Gates

Implementation must begin with experiments for the assumptions that cannot be proven from APIs alone:

1. Extra DHCP identities receive and renew independent leases on representative real Ethernet and Wi-Fi LANs without disturbing the PC lease or host network manager.
2. Proxy ARP successfully delivers inbound traffic through common wired and Wi-Fi access points.
3. ~~Android selects the matching local source address among multiple aliases~~ — tested in Phase 0 and failed on every Android version (decision record 0001); version 1 configures one address.
4. Incoming TCP, UDP, and ICMP packets written to `VpnService` reach Android applications as expected on supported versions.
5. ADB transport remains stable while the VPN owns the Android default route.

Failure of a gate narrows the supported mode or network compatibility. It must not be hidden with an undocumented NAT or proxy fallback.

Decision rules are:

| Gate failure | Required scope change |
|---|---|
| Static bidirectional routing or incoming Android delivery fails | Stop the project architecture; do not proceed |
| Multi-address source selection fails (it did, platform-wide) | One selected interface and one alias per phone; multi-LAN deferred to Model A |
| Real Ethernet DHCP or proxy ARP fails | Do not claim generic Ethernet automatic mode; document only passing environments |
| Real Wi-Fi DHCP or proxy ARP fails | Remove generic Wi-Fi support from version 1 |
| ADB stability fails under default-route VPN | Redesign transport bootstrap before proceeding |
| Protected ADB-stdin bootstrap is unavailable on a target Android build | Exclude that build or redesign bootstrap; never fall back to a command-line secret |

Protocol and single-static-alias development may continue after the static tunnel gate passes. Automatic DHCP with proxy ARP becomes the version 1 primary mode only after one real Ethernet and one real Wi-Fi environment pass acquisition, renewal, inbound reachability, host-manager coexistence, and cleanup.

Outcome (decision 0001, 2026-09-21): Ethernet passed; the Wi-Fi-attached-PC run was deferred. Automatic mode is therefore primary on an Ethernet-attached PC and experimental, `doctor`-gated, on a Wi-Fi-attached PC until that environment is verified.
