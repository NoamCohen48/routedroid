# Routedroid Implementation Plan

## Status (2026-10-03)

| Phase | State |
|---|---|
| 0 Feasibility | Closed (decisions 0001, 0002). Multi-alias source selection failed, so v1 uses one alias per phone |
| 1 Protocol foundation | Done: protocol v1 with fixtures shared by Rust and Kotlin; the app is rebuilt around `DeviceLink` |
| 2 Safe host networking | Done: policy-driven root helper, write-ahead journal, ownership tags, per-session nft table, refcounted sysctls, kill matrix (264 checks) |
| 3 Automatic DHCP | Done: `routedroid-dhcp` (RFC 2131/5227, fuzzed). The helper leases, renews, declines and releases, even after a crash (`dhcp-session.sh`) |
| 4 Interface selection, egress | Done: `routedroid interfaces` shows eligibility and policy. Per-phone policy routing keeps egress on the LAN. Losing the interface ends the session |
| 5 Helper hardening, recovery | Done: sandboxed socket-activated units, `ExecStopPost` cleanup, `doctor [--repair]` with a dry-run default |
| 6 UX, packaging | Done for the host: CLI, TUI, JSON, `install.sh` with uninstall and purge, top-level README. The app shows status, its address and Stop |

Open items against §11 and §12:

- **Hardware matrix (M4).** A leased session on a real phone (Samsung, Android 10) passes
  `integration-tests/vm/phone-session.sh`. It runs through the installed units on a KVM
  guest with a virtual LAN, a dnsmasq router and the phone on USB. Still due: physical LANs
  (consumer and enterprise routers, Wi-Fi) and more phones.
- **ADB reconnect (§11.9).** An unplugged phone ends its connection when the transport
  closes, or at the latest at the 30 s keepalive deadline, and teardown follows. Seen once in the VM lab,
  when the phone dropped off USB mid-session: the lease was released and nothing was left.
  It does not reconnect, and no rig unplugs a phone on purpose yet.
- **Distribution packages and reproducible APKs.** `install.sh` and Gradle builds exist.
  Neither distribution packages nor build reproducibility exist yet.

## 1. Delivery Strategy

Build the smallest end-to-end routed packet path first, prove the platform and network feasibility gates, and only then automate host networking. Each phase has an acceptance gate. Work does not proceed by assuming that DHCP aliasing, proxy ARP, Android source selection, protected bootstrap, or cleanup behavior is uniform.

The planned repository layout is:

```text
android/                 Android application and tests
host/                    Rust workspace for CLI, daemon, and privileged helper
protocol/                Wire protocol specification and shared test vectors
integration-tests/       Linux namespace and hardware test orchestration
.docs/                   Architecture, operations, and decisions
```

## 2. Technology Decisions

### Android

- Kotlin.
- Gradle Kotlin DSL.
- Android `VpnService` and foreground service APIs.
- Coroutines for control lifecycle and bounded packet pumps.
- Direct `FileChannel` or file-descriptor I/O after profiling; avoid per-packet object allocation.
- Minimal UI using the project's chosen Android UI toolkit after the networking core is proven.

### Linux Host

- Rust stable toolchain.
- Tokio for control-plane and ADB stream tasks.
- Netlink library for links, addresses, and routes rather than parsing `ip` output.
- nftables through a structured interface or atomic generated ruleset passed to `nft`; no chains of ad hoc shell commands.
- Raw `AF_PACKET` sockets for isolated DHCP and ARP behavior.
- TUN through `/dev/net/tun`.
- Serde JSON for low-rate control bodies; fixed binary framing for packets.

Exact crate and Android dependency versions are selected during scaffolding and pinned in lockfiles. Dependencies must be maintained, narrowly scoped, and compatible with the required licenses.

## 3. Phase 0: Feasibility Spikes

All packet-forwarding Phase 0 work begins in Linux network namespaces or on a disposable, isolated lab segment. It must not run on a production, office, or otherwise unmanaged LAN. Before the first packet is forwarded, the test harness records route, nftables, sysctl, ADB reverse, and link baselines; installs TUN-ingress source validation plus explicit input and forward restrictions; and registers guaranteed signal/exit cleanup. The harness verifies the baseline after every test.

A minimal supervising network helper is part of Phase 0 before any real Ethernet or Wi-Fi test. It exclusively owns the TUN descriptor and relays packets over bounded Unix `SOCK_SEQPACKET`; the controller never holds a duplicate. Its systemd unit uses `Restart=no`, an independent root-owned `ExecStopPost` cleanup program, and an `ExecStartPre` reconciliation guard. Every mutation journals and `fsync`s intent before execution and completion afterward. Cleanup inspects pending and completed entries; failure leaves the unit failed, and the pre-start guard blocks session or TUN-name reuse until reconciliation succeeds. Real-LAN DHCP tests that do not forward traffic may use only the raw DHCP probe; proxy ARP and inbound tests require the supervisor and cleanup unit.

### 3.1 Minimal Packet Tunnel

Implement throwaway-quality but testable host and Android probes:

- create Android `VpnService` with one address and route;
- create a non-persistent Linux `IFF_TUN | IFF_NO_PI` interface;
- establish one `adb reverse` TCP connection;
- frame one IPv4 packet per message;
- copy packets in both directions; and
- manually install one route, deny-safe nftables rules, and proxy ARP setting inside the isolated environment.

Do not implement production DHCP, UI polish, or full persistence yet.

Acceptance:

- Android can ping the PC and one LAN host through the TUN path;
- the PC and a LAN host can ping the Android VPN address;
- a LAN host can initiate TCP and UDP traffic to test applications on Android;
- packet capture confirms that Linux forwards the original phone address without NAT;
- disconnecting ADB does not cause unbounded memory growth;
- spoofed packets from the phone are dropped before routing when aimed at forwarded and host-local destinations;
- valid-alias traffic reaches explicitly permitted selected-interface host addresses but is dropped for management, container, VPN, and other unselected host addresses;
- TUN reads and writes contain one raw IPv4 packet without a packet-information prefix; and
- teardown restores the recorded route, nftables, sysctl, reverse-map, and link baseline.

The helper acceptance gate sends `SIGKILL` at every mutation boundary: before intent journaling, after the intent `fsync`, after kernel mutation but before completion recording, and after completion `fsync`. It also forces cleanup failure and verifies that the unit remains failed, `ExecStartPre` refuses restart, and neither the session nor TUN name can be reused until reconciliation succeeds.

Packet direction is asserted explicitly:

```text
Android VPN read -> host TUN write -> Linux ingress
Linux TUN read -> Android VPN write -> Android ingress
```

Tests handle interrupted I/O and capture both TUN and physical boundaries. A short TUN write resets the packet path and never retries the suffix; a zero-byte interrupted call may retry. TCP frame writes continue from a partial stream write until the entire frame is sent. Fault injection verifies this distinction.

### 3.2 Multiple Android Addresses

Configure two phone aliases and destination-specific routes on one VPN interface. Compare `/32` aliases plus explicit LAN routes with actual-prefix aliases plus explicit LAN routes. Record `LinkProperties`, observable route state, selected packet sources, and captures. Test on the oldest and newest supported Android versions and at least one intermediate vendor device.

Acceptance:

- traffic to each LAN uses the matching phone source address;
- replies return without NAT or application-specific binding;
- inbound connections to both aliases reach Android;
- default traffic uses the configured primary alias; and
- results are repeatable after VPN re-establishment.

Application tests separately cover:

- ICMP echo behavior;
- TCP listeners bound to wildcard and to each alias;
- UDP request/response sockets bound to wildcard and to each alias;
- connected outbound sockets without explicit source binding; and
- explicitly source-bound sockets as controls.

For every case, record application-observed local and remote addresses independently from packet captures.

If this fails on a platform, classify that platform as single-alias until a non-NAT solution is demonstrated.

### 3.3 DHCP Alias Probe

Build a host-only DHCP experiment that sends a unique client identifier without configuring the lease as a local physical-interface address.

Test at least:

- dnsmasq in a Linux network namespace;
- a common home router over Ethernet;
- a common home or office Wi-Fi access point; and
- one managed network when permission is available.

Every real-network test requires explicit authorization from that network's owner or operator, not merely physical access. Before testing, record DHCP pool capacity where available, use a named unique test identity, cap acquisition attempts and exponential retries, and define whether the resulting lease will be retained or released. Shared real LANs are never used for synthetic address conflicts, DHCPDECLINE tests, or destructive lease exhaustion unless the DHCP administrator explicitly approves them; those cases run in namespaces or a dedicated lab DHCP environment.

Acceptance:

- the PC retains its original lease;
- the extra identity receives a distinct lease;
- renewal and release work;
- INIT-REBOOT restores a persisted valid lease and handles NAK;
- broadcast replies are captured reliably;
- NetworkManager or systemd-networkd is not disturbed; and
- ARP conflict detection, DHCPDECLINE, address announcements, and post-activation conflict withdrawal can be demonstrated;
- unicast raw-frame renewal and release work without assigning the alias locally; and
- VLAN-netdevice operation and packet metadata are either validated or explicitly excluded.

Document incompatible network behavior rather than bypassing network controls.

### 3.4 Protected Bootstrap Probe

Implement the secure bootstrap before testing ADB transport under the VPN default route:

1. Stream a fixed-size, versioned secret record through standard input to `adb shell content write`.
2. Require the exported provider's `android.permission.DUMP` check and explicit shell UID validation.
3. Keep the record only in private volatile memory with a short expiration.
4. Launch the activity with non-secret session and port references.
5. Complete mutual HMAC host authentication before VPN consent, service start, or persistent mutation.

Acceptance:

- the mechanism works on every candidate Android version and vendor family;
- host process command lines, shell arguments, logs, and diagnostics contain no secret;
- provider process death and record expiration fail closed;
- direct hostile application calls to the provider are denied;
- hostile activity launches without a shell-delivered record cause no VPN prompt, service start, or persistent change;
- wrong, replayed, expired, and concurrent handshakes fail safely; and
- both endpoints clear the secret after one successful handshake.

If the protected stdin provider is unavailable on a target build, that build is excluded until a different non-command-line bootstrap is reviewed. Phase 1 productizes this proven mechanism; it does not introduce it for the first time.

### 3.5 Gate Review

Record results in `.docs/decisions/0001-feasibility-results.md`. Confirm or revise the architecture before production implementation. Protocol and static single-alias Phase 1 work may begin after one-address routing and inbound delivery pass. Automatic DHCP/proxy ARP may be called the primary version 1 mode only after one real Ethernet and one real Wi-Fi environment pass acquisition, renewal, inbound reachability, host-manager coexistence, announcements, and cleanup.

The decision record contains a table for every gate with test environment, evidence, result, and mandatory scope reduction. A failed multi-address gate removes multi-alias claims for that Android version. A failed real Wi-Fi gate removes generic Wi-Fi support rather than being hidden as an implementation detail.

## 4. Phase 1: Repository and Protocol Foundation

### Host Deliverables

- Rust workspace and CLI skeleton.
- Structured logging and error taxonomy.
- ADB command wrapper with serial pinning and timeouts.
- Per-device session state machine.
- Requests for helper-owned TUN sessions and asynchronous whole-packet I/O over bounded `SOCK_SEQPACKET`.
- Loopback listener and per-device ADB reverse mapping.
- Collision-free device-port selection, reverse-list ownership checks, and exact reverse-map cleanup.
- Session-secret generation and nonce-based mutual HMAC handshake.

### Android Deliverables

- Application and foreground `VpnService` skeleton.
- VPN permission flow.
- Exported, non-browsable, rate-limited bootstrap activity and non-exported foreground VPN service.
- Shell-UID and `android.permission.DUMP` protected bootstrap content provider that accepts the session secret through `adb shell content write` standard input.
- Protected host transport.
- Configuration validation.
- TUN and transport packet pumps.
- Basic status and stop controls.

### Protocol Deliverables

- `protocol/version-1.md` defining framing and messages.
- Numeric 64 KiB control limit, negotiated packet MTU, absolute 65,535-byte IPv4 limit, legal flags, and state-specific message allowlists.
- Golden frame fixtures consumed by Kotlin and Rust tests.
- Bounds and malformed-input cases.
- Compatibility rule: incompatible major versions fail before VPN activation.

Acceptance:

- one statically configured phone address works end to end;
- Android and Rust pass the same protocol fixtures;
- invalid frame tests cannot trigger large allocations or hangs;
- session authentication rejects incorrect and replayed tokens; and
- Android VPN stop always closes the packet path.

Version 1 accepts USB ADB and Android TLS wireless debugging only. Tests reject legacy network ADB. The mutual HMAC transcript binds both nonces, role labels, version, session, and reverse-port identity; the shared secret is never placed in process arguments or sent as proof and is consumed by both endpoints after one successful handshake. Tests inspect host process arguments, invoke the exported activity from a hostile app, and verify that no VPN prompt, service start, or persistent state appears before host authentication.

## 5. Phase 2: Safe Host Network Configuration

The supervising privileged helper introduced in Phase 0 is retained for all non-isolated tests. The controller is unprivileged; the helper exclusively owns the TUN and network resource lifetime. Its systemd `ExecStopPost` cleanup removes exact journaled resources after helper death and blocks interface-name reuse until successful cleanup.

### Netlink and TUN

- Allocate deterministic but collision-free `phoneN` names.
- Set interface state and MTU.
- Install `/32` alias routes with matching preferred host source addresses.
- Monitor physical link and address changes through netlink.
- Exactly one selected interface per phone in version 1.

### Sysctls

- Test and prefer per-interface forwarding on `phoneN` and selected physical interfaces.
- Treat global `net.ipv4.ip_forward=1` as an explicit operator prerequisite if supported kernels require it; do not toggle it automatically.
- Enable proxy ARP only on selected interfaces.
- Preserve baseline values.
- Record effective `rp_filter` values for `all`, `default`, TUN, and selected interfaces; avoid automatic global changes.
- Reconcile stale state after crashes.

### Firewall

- Create a dedicated nftables table.
- Install a raw-priority (`-300`) prerouting chain that drops every non-session source entering a phone TUN before conntrack and routing.
- Install an input chain at priority `-10` that allows phone traffic only from the correct active alias to an explicit per-session set of selected-interface host addresses, followed by a terminal `iif phoneN drop`.
- Install a forward chain at priority `-10` with policy `accept` and explicit directional paths.
- For phone-to-network paths, require `iif phoneN`, the alias owned by the selected `oif`, and a destination in that interface's permitted set.
- For network-to-phone paths, require the owning `iif`, exact destination alias, `oif phoneN`, and a source in that interface's permitted set.
- Use `0.0.0.0/0` permitted sets on the selected interface so Internet traffic and replies pass.
- Constrain established and related rules to the same exact session path.
- Install priority `-10` postrouting/output validation with terminal drops so only active destination aliases leave through a phone TUN.
- Add terminal forward drops for every unmatched `iif phoneN` or `oif phoneN` path.
- Deny phone access to unselected interfaces.
- Deny phone-to-phone forwarding by default.
- Apply and remove rule updates atomically.
- Generate deterministic ruleset fixtures with explicit hooks, numeric priorities, sets, and ordering.
- Document that Routedroid accept rules cannot override later host-firewall drops.
- Compile and load generated fixtures on every supported kernel/nftables baseline.

Acceptance:

- two phones cannot use each other's source addresses;
- spoofing toward forwarded and host-local destinations is dropped before route classification;
- valid-alias probes to selected host addresses follow explicit policy while probes to every unselected host address are dropped;
- no host service receives traffic addressed to a phone alias;
- unselected interfaces remain unreachable from the phone;
- existing nftables rules are not deleted or rewritten;
- repeated start and stop leaves the original route, firewall, and sysctl state;
- controller termination closes the packet channel and triggers helper plus `ExecStopPost` cleanup; and
- `SIGKILL` of the helper while the controller remains alive removes the exclusively owned TUN immediately and runs independent cleanup before restart.

## 6. Phase 3: Automatic DHCP

### DHCP State Machine

Implement per-device, per-interface states:

```text
Init -> Selecting -> Requesting -> Bound
Init -> InitReboot -> Bound or Init
Bound -> Renewing -> Rebinding -> Bound
Any lease state -> Expired or Declined -> Init
```

Required protocol behavior:

- randomized transaction IDs;
- stable unique option 61 client identity;
- broadcast reply request;
- exact raw Ethernet, IPv4, UDP, BOOTP, and DHCP fields and checksums for every state;
- offer selection policy;
- requested-address and server-identifier handling;
- subnet, router, classless-route, DNS, lease, T1, and T2 parsing;
- exponential retry with jitter and global timeout;
- renewal, rebinding, decline, and release;
- server NAK handling, INIT-REBOOT, next-hop MAC resolution, interface-index binding, and VLAN-netdevice metadata; and
- strict length and option validation.

### Lease Activation Transaction

Activate a lease as one transaction:

1. Validate that the offered address belongs to the selected interface's current subnet.
2. Perform ARP conflict probes.
3. Persist lease metadata atomically.
4. Install firewall rules in a disabled/not-ready deny state.
5. Install the host route.
6. Enable proxy ARP reference and required per-interface forwarding.
7. Include the lease in the next Android configuration.
8. After Android reports `VPN_READY`, send ARP announcements and atomically enable session forwarding.

Rollback all session-created steps if any operation fails.

### Network Manager Coexistence

- Use raw sockets and unique DHCP transactions.
- Never reconfigure the physical interface's existing address.
- Test while NetworkManager and systemd-networkd independently own the PC lease.
- Filter only Routedroid DHCP transactions.
- Verify that manager restart or reload does not alter the alias, route, DNS, connectivity-check result, or physical-interface management state.
- Report port-security or DHCP-snooping failures without retry storms.

Acceptance:

- `routedroid start` needs no manually selected phone address on a compatible LAN;
- leases survive normal renewal;
- a changed lease safely reconfigures Android and host routes;
- expiration removes reachability before address reuse;
- duplicate-address responses cause decline and reacquisition;
- lease reuse updates stale neighbor caches through announcements;
- a conflict detected after activation immediately withdraws reachability; and
- unsupported LANs fail with a clear fallback recommendation.

## 7. Phase 4: Interface Selection and Egress Policy

Multiple selected interfaces were removed from version 1 after the Phase 0
§3.2 gate failed on every tested Android version (decision record 0001):
Android installs VPN routes without a preferred source, so a phone with several
aliases always initiates traffic from the first one, and the operator cannot add
router routes to compensate. Version 1 supports one operator-selected interface.

### Configuration

```text
interface = eth0
dns_policy = interface
```

Eligibility rules exclude by default:

- loopback;
- TUN/TAP and Routedroid interfaces;
- container and bridge interfaces;
- interfaces without usable IPv4 configuration; and
- interfaces explicitly denied by policy.

The user must explicitly select the interface; `routedroid interfaces` lists
candidates with eligibility and rejection reasons.

### Host Egress Policy

- Bind the alias to the selected interface in the firewall.
- If the host's default route does not leave through the selected interface,
  install a source-policy rule and table for the alias (connected route plus
  the DHCP-learned gateway) from a reserved range, journaled.
- Never fall through to an unrelated host default.
- Handle selected-interface loss by disabling forwarding and reporting; no
  automatic move to another interface.
- Verify with `ip route get DEST from ALIAS` before readiness and after netlink changes.

Acceptance:

- hosts on the selected LAN initiate TCP, UDP, and ICMP traffic to the phone alias;
- phone traffic to the LAN and the Internet carries the alias;
- Internet egress uses the selected interface even when the host default route points elsewhere;
- unselected interfaces are unreachable from the phone; and
- selecting an ineligible interface fails with an actionable message.

Later: multi-interface mode with inbound-only secondary LANs, or Model A
(routed phone subnet) where routers can be configured.

## 8. Phase 5: Privilege Helper Hardening and Recovery

Harden the split introduced by the Phase 0 supervisor. The helper remains the exclusive TUN owner and exchanges complete packets with the controller over bounded Unix `SOCK_SEQPACKET`:

```text
routedroid             unprivileged controller and ADB owner
routedroid-net-helper  privileged, typed network-operation broker
```

Helper API operations include:

- create/delete owned TUN;
- add/delete exact owned route;
- apply/delete exact nftables session;
- acquire/release raw DHCP and ARP socket for an allowed interface;
- acquire/release forwarding and proxy ARP references; and
- inspect owned state.

Requests are authenticated over a root-owned Unix socket using peer credentials. The helper validates interface names, indexes, address ownership, route scope, and session identifiers.

Recovery requirements:

- persist and `fsync` mutation intent before every external change, then persist and `fsync` completion;
- use idempotent operations;
- identify resources by reserved names and nftables comments;
- reconcile pending and completed entries by inspecting only resources proven to be Routedroid-owned;
- never restore a sysctl over a concurrent administrator change; and
- provide `routedroid doctor --repair` with a dry-run default.

Acceptance:

- the controller runs without broad network capabilities;
- malformed Android or controller input cannot request arbitrary routes or firewall rules;
- kill tests at every write-ahead mutation point converge to a clean state;
- cleanup failure blocks restart and resource-name reuse until reconciliation; and
- repair output names every proposed change.

The service manager invokes the independent cleanup program after helper failure and only restarts after cleanup succeeds. The non-persistent TUN disappears when the helper's descriptor closes; cleanup removes journaled nftables state, routes, policy rules, proxy references, and sysctl changes. The controller cannot keep the TUN alive. No active rules are assumed to become deny-safe merely because a process crashed.

## 9. Phase 6: User Experience and Packaging

### Android UX

- Explain why VPN permission is required.
- Show connected host, assigned aliases, primary network, and packet status.
- Show actionable errors for permission loss, ADB loss, and rejected configuration.
- Provide a stop action from the foreground notification.
- Avoid claiming Ethernet bridging or guaranteed compatibility with restricted LANs.

### Host UX

- Interface list includes address, subnet, link type, eligibility, and rejection reason.
- Start output clearly shows DHCP acquisition and each installed phone alias.
- Status distinguishes tunnel, VPN, lease, route, proxy ARP, and firewall health.
- Doctor checks ADB authorization, capabilities, TUN, nftables, sysctls, firewall conflicts, and DHCP compatibility.
- JSON output is available for automation.

### Packaging

- Reproducible Android APK builds.
- Host binary packages for supported Linux distributions.
- udev and helper-service configuration with least privilege.
- Uninstall removes only Routedroid-owned persistent resources.

Acceptance:

- a new operator can connect one phone without selecting an address manually;
- all privileged prompts and network mutations are visible;
- uninstall and stop leave no active forwarding path; and
- documentation matches actual command behavior.

## 10. Test Strategy

### Unit Tests

- Frame parser and encoder golden vectors.
- IPv4 header and length validation.
- DHCP options, malformed packets, timers, and state transitions.
- Route and firewall desired-state generation.
- Lease persistence and schema migration.
- Session state machine cancellation and retries.

### Property and Fuzz Tests

- Arbitrary frame streams never exceed configured allocation bounds.
- DHCP and IPv4 parsers never panic on arbitrary input.
- Encode/decode round trips preserve valid protocol messages.
- Reconciliation remains idempotent across repeated partial states.

### Linux Namespace Integration Tests

Use network namespaces, veth pairs, nftables, and dnsmasq to model:

```text
phone namespace <-> host namespace <-> LAN namespace <-> router namespace
```

Tests cover:

- DHCP alias acquisition;
- proxy ARP neighbor resolution;
- inbound and outbound TCP, UDP, and ICMP;
- unchanged packet addresses;
- a second, unselected physical subnet that must remain unreachable;
- firewall isolation;
- link loss;
- lease renewal and replacement; and
- cleanup after injected failures.

Tests run with `rp_filter` strict, loose, and disabled where supported and verify the effective `all` plus per-interface behavior. MTU tests negotiate the same Android/TUN value, reject oversized frames, verify route MTU, and exercise DF packets plus returned ICMP fragmentation-needed messages.

Namespace tests do not replace physical Wi-Fi testing because access points may enforce station-specific policy.

### Android Automated Tests

- VPN authorization and lifecycle instrumentation tests.
- Protocol compatibility tests using golden fixtures.
- Reconfiguration tests for address changes.
- Foreground-service and process-restart behavior.
- Packet-path tests on emulator where supported.

Use Maestro for end-to-end UI permission and status flows after the basic app exists. Network correctness is verified through packet tests rather than screenshots.

### Hardware Matrix

At minimum:

- oldest supported Android API emulator/device;
- current Android API emulator/device;
- one Samsung or other materially customized Android device;
- USB ADB and Android TLS wireless debugging;
- wired Ethernet host LAN;
- ordinary Wi-Fi host LAN;
- a second host LAN present but unselected; and
- one intentionally incompatible managed LAN to verify safe failure.

## 11. End-to-End Acceptance Criteria

Version 1 is ready only when all applicable criteria pass:

1. A compatible DHCP server assigns a phone alias without manual address selection.
2. The PC's own lease and connectivity remain unchanged.
3. Packet capture shows the phone alias end to end with no PC-side NAT or replacement sockets.
4. LAN hosts initiate TCP, UDP, and ICMP traffic to the phone.
5. The phone initiates traffic to the selected LAN and the Internet with its alias, subject only to the upstream router's NAT.
6. Traffic never leaves through an unselected interface.
7. Multiple phones receive isolated addresses and packet paths.
8. Spoofed phone source addresses and unselected interfaces are blocked.
9. DHCP renewal, ADB reconnect, interface loss, VPN revocation, and lease expiration fail safely.
10. Normal stop, forced termination, and repair leave no unintended routes, firewall rules, or sysctl changes.
11. Restricted networks fail clearly without address guessing or policy bypass.

## 12. Milestones

### M0: Feasibility Proven

- Static bidirectional raw packet tunnel.
- Inbound Android sessions.
- Multiple-address source-selection report (done: failed; single alias adopted).
- DHCP alias compatibility report.

### M1: Single-LAN Developer Prototype

- Authenticated protocol.
- One manually configured alias.
- Safe route, proxy ARP, and firewall setup.

### M2: Automatic Single-LAN Alpha

- DHCP acquisition and renewal.
- Transactional activation and cleanup.
- Basic CLI and Android status UI.

### M3: Egress and Isolation Beta

- Interface eligibility and selection UX.
- Egress policy when the host default route is elsewhere.
- Link-change handling and isolation tests.

### M4: Hardened Version 1

- Hardened privilege separation and supervised cleanup.
- Crash reconciliation.
- Packaging, diagnostics, compatibility documentation, and full hardware matrix.

## 13. Risks and Mitigations

| Risk | Consequence | Mitigation |
|---|---|---|
| DHCP server permits only one identity per station | No automatic alias | Detect and offer documented manual fallback; do not guess silently |
| DHCP snooping or ARP inspection rejects proxy identity | No inbound reachability | Compatibility diagnosis; mark interface unsupported |
| Android chooses wrong source among aliases | Secondary LAN replies fail | Confirmed in Phase 0 on all versions; version 1 uses one alias |
| Existing host firewall drops forwarding | Tunnel appears partially broken | Doctor diagnostics and narrowly documented integration rules |
| ADB disconnect causes head-of-line stalls | Temporary packet loss | Bounded queues, reconnect state, metrics, no unbounded buffering |
| VPN re-establishment changes addresses | Existing sessions drop | Preserve leases; transactional reconfiguration; report disruption |
| Host process crashes after network mutation | Stale forwarding state | Supervising helper cleanup, deny-safe residual rules, and journaled reconciliation |
| Host default route not on the selected interface | Wrong egress | Alias source-policy rule to the selected interface's gateway |
| Wildcard PC services claim phone address | Inbound reaches PC | Route alias through TUN; never add it as a local host address |
| Global forwarding enables unrelated paths | Host network exposure | Prefer per-interface forwarding; require operator-managed global forwarding only when necessary; install deny rules first |
| DHCP lease is cached with an old MAC | Temporary inbound failure | Send ARP announcements after safe activation and monitor conflicts |

## 14. Documentation Deliverables

Before version 1:

- architecture and protocol specifications;
- supported and unsupported network behavior;
- installation and privilege model;
- single-interface operation and the multi-LAN limitation;
- DHCP/manual fallback procedure;
- firewall coexistence guide;
- troubleshooting and packet-capture guide;
- security model;
- test matrix and known incompatibilities; and
- clean uninstall and recovery procedure.
