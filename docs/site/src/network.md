# Addresses, DNS and egress

## Addresses

**Leased.** Without `--phone-ip`, the helper leases an address for the phone from the LAN's
DHCP server, on the LAN interface. The client identifier is
`routedroid:<device id>:<PC MAC>`, so the same phone tends to get the same address back.

The helper renews the lease for as long as the connection lasts, and releases it at the
end, even after a crash. If the lease is lost, or another station claims the address, the
connection ends with the reason.

The policy must allow DHCP on that interface (`dhcp = true`). If it also lists
`phone_addresses`, a lease must fall inside them.

**Chosen.** With `--phone-ip`, the address must lie in the interface's `phone_addresses`
in the [policy](policy.md). The helper refuses an address that:

- answers ARP (someone has it);
- is the gateway, or one of the PC's own addresses;
- is the network or broadcast address;
- is already routed.

## DNS

By default, the phone uses the lease's DNS servers, or else the LAN's gateway.

- `--dns 9.9.9.9` gives it that server instead (repeat it for more).
- `--no-dns` gives it none.

## Egress

The phone's traffic always leaves through its LAN: through the lease's router, else through
the LAN interface's own default route. This holds even when the PC's default route goes
elsewhere, such as a VPN or another uplink. With no gateway, the phone reaches that LAN and
nothing else.

The phone cannot reach the PC's other interfaces, other phones, or addresses it was not
given.

There is no NAT: on the LAN, the phone's packets carry its own address.

## Watching packets

```sh
tcpdump -ni phone0                        # what the phone sends and receives
tcpdump -ni eno1 host 192.168.1.57        # the same packets on the LAN
ip rule; ip route show table all          # the phone's egress
```
