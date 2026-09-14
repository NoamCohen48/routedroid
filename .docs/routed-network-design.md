# Routed Android Networking over ADB

## Goal

Connect an unrooted Android phone to a Linux PC over ADB and make the phone a reachable IP host on networks available to the PC.

The intended design is not a connection proxy. It transports complete IP packets between an Android `VpnService` interface and a Linux TUN interface:

```text
Android applications
        |
Android VpnService (IP/TUN)
        |
        | raw IPv4/IPv6 packets over ADB
        |
Linux TUN interface (phone0)
        |
Linux routing and firewall
        |
PC LAN interfaces / router / Internet
```

Android will know that a VPN is active. That does not prevent the phone from acting as a routed host.

## What This Design Provides

- The phone can have one or more dedicated IP addresses.
- Complete IPv4 or IPv6 packets can cross the tunnel.
- TCP, UDP, ICMP, and other IP protocols are not replaced by PC sockets.
- LAN machines can initiate connections to services listening on the phone when return routing and firewalls permit it.
- The phone can initiate connections to LAN machines and the Internet.
- The PC can connect directly to the phone's tunnel or LAN address.
- The phone can use more than one network attached to the PC.

## What It Does Not Provide

Android `VpnService` is a Layer-3 IP interface, not an Ethernet TAP interface. Consequently, the phone does not become a literal Layer-2 Ethernet peer:

- Android does not have a LAN-visible MAC address through this tunnel.
- Android cannot answer Ethernet ARP requests directly.
- Ethernet frames and arbitrary Layer-2 protocols cannot cross the tunnel.
- Broadcast-based discovery does not automatically cross routed boundaries.
- mDNS, SSDP, and similar discovery protocols may require a reflector or relay on the PC.
- Applications can still detect that Android is using a VPN.

For normal IP communication, including incoming TCP and UDP sessions, these limitations are usually not important.

## Why This Is Technically Possible

Android's `VpnService` creates an IP interface accessible through a file descriptor. The VPN application:

- reads outgoing IP packets selected by the configured VPN routes;
- writes incoming IP packets into Android as if the interface had received them;
- can assign IPv4 and IPv6 addresses to the VPN interface; and
- can install default routes so application traffic enters the tunnel.

Linux TUN provides the corresponding IP-packet interface. A host program transfers each packet between the Android VPN file descriptor and `phone0` over ADB. Linux then routes the packets instead of terminating their TCP or UDP sessions.

The tunnel's own ADB transport must be excluded from the Android VPN path or protected with `VpnService.protect()` where applicable. Otherwise, the transport can be routed back into itself.

## Method 1: Address from the Existing LAN with Proxy ARP

This method makes the phone appear to use an address from the PC's existing LAN subnet.

### Example

```text
LAN:                192.168.1.0/24
LAN router:         192.168.1.1
PC Ethernet/Wi-Fi:  192.168.1.20
Phone:              192.168.1.50
Linux tunnel:       phone0
```

The address `192.168.1.50` must be reserved for the phone and excluded from the LAN's DHCP pool.

Two Android address configurations must be compared during the initial feasibility tests:

```text
Candidate A:   192.168.1.50/32
Candidate B:   192.168.1.50/24
Default route: 0.0.0.0/0
LAN route:     192.168.1.0/24
```

`/32` most clearly represents a routed point-to-point path. The real LAN prefix may produce better Android source-address selection when several aliases share one VPN interface. Routedroid must test both representations and adopt one consistently based on observed Android routes and packet sources; this document does not assume the result.

Linux is conceptually configured with:

```bash
sudo ip link set phone0 up
sudo ip route add 192.168.1.50/32 dev phone0
sudo sysctl -w net.ipv4.conf.phone0.forwarding=1
sudo sysctl -w net.ipv4.conf.eth0.forwarding=1
sudo sysctl -w net.ipv4.conf.eth0.proxy_arp=1
```

`eth0` is an example. The real interface might be named `wlan0`, `enp3s0`, or something else. Production configuration should be applied through the system's persistent network configuration rather than temporary commands.

The Linux forwarding firewall must also permit traffic between `phone0` and the LAN interface. Some kernels or host configurations may require global `net.ipv4.ip_forward=1`; this should be an explicit administrator-managed prerequisite because it affects the whole host. Reverse-path filtering may need loose mode on relevant interfaces if it rejects valid policy-routed tunnel paths, and its effective `all` plus per-interface settings must be checked.

### Outbound Packet Flow

When the phone contacts a LAN server:

```text
192.168.1.50:40000 -> 192.168.1.30:443
```

1. Android places the original IP packet in its VPN interface.
2. The Android tunnel application sends the packet over ADB.
3. The PC tunnel process writes the packet to `phone0`.
4. Linux forwards it through the physical LAN interface.
5. The server sees the real source address `192.168.1.50`.

The PC does not translate the source address or create a replacement TCP connection.

### Inbound Packet Flow

Suppose a laptop at `192.168.1.30` connects to a phone service:

```text
192.168.1.30:50000 -> 192.168.1.50:8080
```

Because both addresses are in `192.168.1.0/24`, the laptop broadcasts an ARP request asking for the MAC address of `192.168.1.50`. Android cannot receive that Ethernet request. Proxy ARP makes the PC answer with the PC's MAC address on the phone's behalf.

The laptop sends the packet to the PC's MAC address. Linux finds the `/32` route through `phone0`, and the tunnel delivers the unchanged packet to Android. Android can then deliver it to an application listening on port `8080`.

### Advantages

- The phone has an address that looks like part of the existing LAN.
- Other LAN hosts do not need individual static routes.
- The LAN router usually does not require a new route.
- LAN hosts can initiate connections to the phone without port forwarding.
- The PC retains its own address and does not need to own the phone address locally.

### Disadvantages and Constraints

- The PC must provide proxy ARP.
- The selected phone address must not be used by another host.
- Some enterprise, Wi-Fi, or isolated-client networks may block the required forwarding behavior.
- This is still routed Layer 3 behind the scenes, not an Ethernet bridge.
- Discovery protocols that depend on broadcasts or multicasts may need explicit relays.
- Each phone consumes an address from the existing LAN.

### Automatic Address Allocation with DHCP

The phone address does not have to be selected statically. Because Android `VpnService` does not expose Ethernet frames, Android cannot directly send a normal DHCP broadcast through the VPN interface. Instead, Routedroid can request an additional DHCP lease from the existing LAN DHCP server on the phone's behalf.

The automatic process is:

1. Routedroid detects the PC's eligible physical network interfaces.
2. A host-side DHCP client requests one additional lease for the phone on each selected interface.
3. The existing LAN DHCP server selects the addresses.
4. The PC sends the assigned addresses, prefixes, routes, DNS servers, and lease times to Android over the ADB control connection.
5. Android adds the addresses and routes to its `VpnService` interface.
6. The PC installs a `/32` route for each phone address through `phone0` and enables proxy ARP on the corresponding physical interface.
7. Routedroid renews each lease while the phone remains connected and removes the address, route, and proxy state when the lease expires or the phone disconnects.

For example, allocation might produce:

```text
eth0 network:         192.168.10.0/24
PC address:           192.168.10.20
DHCP phone address:   192.168.10.74

eth1 network:         172.16.20.0/24
PC address:           172.16.20.20
DHCP phone address:   172.16.20.113
```

The PC acts as an additional DHCP client or DHCP proxy. It must not run a competing DHCP server on the physical LAN. A second DHCP server could answer other machines and disrupt the entire network.

Every phone and physical interface should use a stable, unique DHCP client identifier, for example:

```text
routedroid-phone-DEVICE123-eth0
routedroid-phone-DEVICE123-eth1
```

This allows a DHCP server that supports multiple client identities to maintain independent leases for the PC and phone. Routedroid should retain each lease across short reconnects, renew it according to the DHCP timers, and release it when appropriate.

Automatic allocation depends on the existing network accepting this arrangement. Some networks enforce one lease per physical client or bind DHCP, ARP, and switch state to a specific MAC address. Possible restrictions include:

- one DHCP lease per Ethernet or Wi-Fi station;
- DHCP snooping;
- dynamic ARP inspection;
- Wi-Fi client isolation;
- proxy ARP restrictions; and
- limits on multiple IP identities behind one physical interface.

Routedroid should therefore prefer automatic DHCP allocation but support manual configuration as a fallback. A manually selected address should be checked for an existing ARP owner before use, but ARP probing alone cannot reserve it against a later DHCP assignment. Manual addresses are only dependable when the user knows that they are outside the DHCP pool or otherwise reserved.

With multiple automatically assigned addresses, incoming traffic is straightforward because each destination address identifies the intended subnet. For phone-initiated traffic, Android must select the source address that belongs to the destination subnet. Routedroid should install a destination-specific VPN route for every attached LAN, in addition to any selected default route, and verify source-address selection on every supported Android version.

## Method 2: Separate Routed Phone Subnet

This method gives the phone an address from a dedicated subnet behind the PC. It is ordinary routing and does not require NAT.

### Example

```text
Existing LAN:       192.168.1.0/24
LAN router:         192.168.1.1
PC LAN address:     192.168.1.20

Phone subnet:       10.77.0.0/24
PC tunnel address:  10.77.0.1
Phone address:      10.77.0.2
```

The Android VPN is conceptually configured with:

```text
Address:       10.77.0.2/24
Default route: 0.0.0.0/0
```

Linux is conceptually configured with:

```bash
sudo ip address add 10.77.0.1/24 dev phone0
sudo ip link set phone0 up
sudo sysctl -w net.ipv4.conf.phone0.forwarding=1
sudo sysctl -w net.ipv4.conf.eth0.forwarding=1
```

The LAN router receives this static route:

```text
Destination: 10.77.0.0/24
Next hop:    192.168.1.20
```

The Linux firewall must permit forwarding between `phone0` and the physical LAN interface.

### Outbound Packet Flow

When the phone contacts a laptop, the packet remains:

```text
10.77.0.2:40000 -> 192.168.1.30:443
```

Linux forwards the packet without changing either address. The laptop sees `10.77.0.2` as the source.

The reply is sent to the laptop's default router. The router's static route sends `10.77.0.2` traffic to `192.168.1.20`, and the PC routes it through `phone0` to Android.

### Inbound Packet Flow

A laptop can initiate a connection directly:

```text
192.168.1.30:50000 -> 10.77.0.2:8080
```

The laptop sends this packet to its normal default router. The router uses its static route to forward it to the PC. The PC sends it through the ADB tunnel to the phone. No port mapping is involved.

### Why This Is Not NAT

Routing preserves the packet addresses:

```text
Before PC: 10.77.0.2:40000 -> 192.168.1.30:443
After PC:  10.77.0.2:40000 -> 192.168.1.30:443
```

NAT would rewrite the packet:

```text
Before NAT: 10.77.0.2:40000 -> 192.168.1.30:443
After NAT:  192.168.1.20:51000 -> 192.168.1.30:443
```

The routed method requires a return route. If no route can be added to the LAN router, masquerading could provide outbound access, but that variant would be NAT and unsolicited inbound access would require port mappings.

### Advantages

- It is conventional, explicit IP routing.
- It does not require proxy ARP.
- It scales cleanly to multiple phones.
- It avoids consuming addresses from existing LAN subnets.
- Every phone keeps a stable, independently reachable address.

For example:

```text
PC tunnel: 10.77.0.1
Phone 1:   10.77.0.2
Phone 2:   10.77.0.3
Phone 3:   10.77.0.4
```

### Disadvantages and Constraints

- The LAN router must have a route to the phone subnet, or equivalent routes must be installed on every LAN host that needs access.
- The phone does not have an address from the existing `192.168.1.0/24` subnet.
- Broadcast-based discovery does not cross the routed boundary automatically.
- Networks outside the LAN also need a route if they must initiate connections to the phone.

## Comparison of the Two Methods

| Property | Existing LAN address with proxy ARP | Separate routed subnet |
|---|---|---|
| Example phone address | `192.168.1.50` | `10.77.0.2` |
| Address appears in existing subnet | Yes | No |
| NAT required | No | No |
| Proxy ARP required | Yes | No |
| Router static route required | Usually no | Yes |
| LAN can initiate connections | Yes | Yes |
| Scales to many phones | Moderately | Well |
| True Layer-2 membership | No | No |

Method 1 is closest to making the phone appear to be on the PC's existing LAN. Method 2 is usually cleaner when the LAN router can be configured.

## Difference from Gnirehtet

Gnirehtet captures raw IPv4 packets on Android, but its PC relay does not route those packets onto the network. Instead, it interprets each phone flow and opens a new host socket for it.

Conceptually, Gnirehtet does this:

```text
Android TCP/UDP flow
        |
Gnirehtet relay terminates or emulates the flow
        |
New TCP/UDP socket created by the PC
        |
Destination
```

The routed design does this:

```text
Original Android IP packet
        |
ADB packet tunnel
        |
Linux IP forwarding
        |
Destination
```

| Property | Routed tunnel | Gnirehtet |
|---|---|---|
| PC creates replacement network sockets | No | Yes |
| Original phone packets are forwarded | Yes | No, they are interpreted by the relay |
| Phone has an independently reachable address | Yes | No |
| LAN can initiate arbitrary connections to phone | Yes, when routed and permitted | No by default |
| TCP/UDP source identity | Phone address | PC relay socket |
| Other IP protocols | Possible | Primarily TCP/UDP relay behavior |
| IPv6 architecture | Possible | Gnirehtet is IPv4-only |
| Similar to NAT | No, unless NAT is explicitly added | Yes, documented as similar to a port-restricted cone NAT |

Gnirehtet is useful as a reference for Android VPN setup, ADB transport, packet framing, and lifecycle handling. Its relay layer must be replaced by an IP tunnel and router for this project.

## PC Connected to Multiple Subnets

The phone can access and be reachable from multiple subnets connected to the PC. There are two useful models.

### Model A: One Phone Subnet Routed to Every PC Network

This is the recommended model for multiple networks.

Example host:

```text
PC office interface:  192.168.10.20/24 on eth0
PC lab interface:     172.16.20.20/24 on eth1
PC phone tunnel:      10.77.0.1/24 on phone0
Phone:                10.77.0.2/24
```

The PC already has connected routes to the office and lab networks, so phone traffic can be forwarded to both:

```text
Phone 10.77.0.2 -> PC -> 192.168.10.0/24
Phone 10.77.0.2 -> PC -> 172.16.20.0/24
```

For connections in the other direction, each network needs a return route:

```text
Office router: 10.77.0.0/24 via 192.168.10.20
Lab router:    10.77.0.0/24 via 172.16.20.20
```

If hosts on a subnet use some other gateway and that gateway lacks this route, replies will go to the wrong place and the connection will fail. Installing the route on each subnet's gateway is preferable to installing it separately on every endpoint.

The Linux firewall can control which networks the phone may use. For example, it can allow phone access to the lab network while denying access to the office network, without changing the tunnel design.

This model gives the phone one stable identity, `10.77.0.2`, across all attached networks.

If the PC has more than one Internet/default route, Linux will choose one according to its routing metrics unless policy routing is configured. Policy routing can deliberately select an egress interface based on the phone source address, destination network, firewall mark, or other criteria. Merely connecting the PC to several networks does not make the phone use all default routes simultaneously.

### Model B: One Phone Address from Each Existing Subnet

If the phone must appear to have an address in every existing subnet, the Android VPN can be assigned multiple addresses.

Example:

```text
PC eth0:               192.168.10.20/24
Phone office address:  192.168.10.50/32

PC eth1:               172.16.20.20/24
Phone lab address:     172.16.20.50/32
```

Linux routes both host addresses into the same tunnel:

```bash
sudo ip route add 192.168.10.50/32 dev phone0
sudo ip route add 172.16.20.50/32 dev phone0
sudo sysctl -w net.ipv4.conf.eth0.proxy_arp=1
sudo sysctl -w net.ipv4.conf.eth1.proxy_arp=1
```

Android's VPN configuration adds both addresses. The PC answers proxy ARP for the appropriate phone address on each physical interface.

This permits an office machine to use `192.168.10.50` and a lab machine to use `172.16.20.50`. However, it is more complex than using one routed phone subnet:

- Source-address selection must be predictable when the phone initiates traffic.
- Firewall rules must prevent unintended forwarding between sensitive networks.
- Every address must be reserved outside its subnet's DHCP pool.
- Proxy ARP must work on every involved network.
- Discovery protocols still require relays because Android is not actually attached at Layer 2.

If the phone only needs to communicate with every subnet, Model A is simpler. Use Model B only when the phone specifically needs a local-looking address in each subnet.

### Overlapping Subnets

Ordinary routing becomes ambiguous if two PC interfaces lead to overlapping address ranges, such as two unrelated networks both using `192.168.1.0/24`. Linux cannot select the intended interface from the destination address alone.

Supporting overlapping networks requires additional isolation and policy, such as network namespaces, VRFs, policy routing with marks, or address translation. It should not be treated as the normal multi-interface case.

## Internet Address and NAT Considerations

Being independently reachable on a private LAN does not automatically remove the LAN router's Internet NAT.

For example:

```text
Phone -> ADB tunnel -> PC routing -> home router NAT -> Internet
```

In that topology, the phone is not NATed by the PC, but both the PC and phone are still behind the home router's NAT.

To give the phone non-NATed Internet identity, the upstream network must provide an address that can be routed to it:

- a public IPv4 address routed through the PC;
- a globally routed IPv6 address or prefix; or
- a public address routed from a VPS through a tunnel such as WireGuard.

An address cannot simply be invented or selected from the Internet. The ISP, VPS, or upstream router must route it toward the PC.

If an extra public address terminates on the PC rather than being routed directly to the phone, the PC can use static one-to-one NAT for all traffic. This is more capable than Gnirehtet and can support inbound connections, but it is still NAT. A directly routed public address preserves the phone's address end to end.

## Linux Multiple-Address Behavior

Linux routinely supports multiple interfaces, addresses, routes, and routing tables. This does not inherently break software. Problems usually come from ambiguous configuration:

- A PC service bound to `0.0.0.0` or `::` listens on all addresses locally owned by the PC.
- Outbound PC traffic may select an unexpected source address when routes are ambiguous.
- Strict reverse-path filtering may reject valid asymmetric tunnel traffic.
- Firewall policies may permit unwanted forwarding between interfaces.

The phone addresses should normally be routed through `phone0`, not added as ordinary local addresses on the PC's physical interfaces. Explicit routes and firewall rules keep PC traffic and phone traffic separate.

## Implementation Requirements

A production implementation needs more than packet copying:

- Android VPN permission and foreground-service lifecycle handling.
- IPv4 and preferably IPv6 packet support.
- ADB transport setup, framing, reconnection, and failure detection.
- Host-side DHCP lease acquisition, renewal, release, and unique client identities when automatic Method 1 allocation is enabled.
- Protection against routing the tunnel transport into itself.
- Linux TUN creation and cleanup.
- IP forwarding and narrowly scoped firewall rules.
- Proxy ARP or upstream route management, depending on the selected method.
- Correct MTU selection and Path MTU Discovery behavior.
- Backpressure and bounded packet queues.
- DNS configuration.
- Multiple-phone address allocation and isolation if required.
- Restoration of host networking settings when the tunnel stops.

Security should default to denying forwarding until the required phone-to-network paths are explicitly enabled.

## Recommended Starting Architecture

For a controlled network where the router can be configured, a dedicated phone subnet is technically cleaner:

```text
PC phone0: 10.77.0.1/24
Phone:     10.77.0.2/24
LAN route: 10.77.0.0/24 via the PC
```

It is easier to reason about, supports multiple phones, and avoids proxy ARP. Routedroid's version 1 target is nevertheless automatic DHCP aliases with proxy ARP because its deployment assumption is that router configuration is unavailable. The dedicated routed subnet remains an optional future mode for controlled networks.

## References

- [Android `VpnService`](https://developer.android.com/reference/android/net/VpnService)
- [Android `VpnService.Builder`](https://developer.android.com/reference/android/net/VpnService.Builder)
- [Linux kernel TUN/TAP documentation](https://docs.kernel.org/networking/tuntap.html)
- [Linux kernel IP sysctl documentation](https://docs.kernel.org/networking/ip-sysctl.html)
- [Gnirehtet developer documentation](https://github.com/Genymobile/gnirehtet/blob/master/DEVELOP.md)
- [RFC 3022: Traditional NAT](https://www.rfc-editor.org/rfc/rfc3022)
