# The helper policy

`/etc/routedroid/helper.toml` says which LAN interfaces may carry phones, and which
addresses phones may take there. The root helper reads it on every start, and no client can
override it. [`sudo routedroid setup`](setup.md) writes it for you.

As installed, it allows nothing.

## Format

```toml
[[interface]]
name = "eno1"
dhcp = true                                  # phones may lease an address here
phone_addresses = ["192.168.1.200/29"]       # and may ask for one of these (bounds leases too)

[[interface]]
name = "enp5s0"
phone_addresses = ["10.0.0.64/28"]
```

One `[[interface]]` table per interface that may carry phones:

| Key | Type | Means |
|---|---|---|
| `name` | string | the interface name, as `ip link` shows it |
| `dhcp` | true or false (default false) | phones may lease an address from the LAN's DHCP server |
| `phone_addresses` | list of CIDR blocks (default empty) | addresses a phone may ask for with `--phone-ip`; with `dhcp`, a lease must fall inside them too |

An interface missing from the file, or one with neither key, admits no phone. Unknown keys
are an error.

## Rules for the file

- It must be a regular file, owned by root, and not writable by group or others. Otherwise
  the helper refuses every session.
- A file that does not parse refuses every session too.
- Changes take effect at the next start. Running connections keep going.

`routedroid interfaces` shows what the helper makes of it: each interface, and whether a
phone may join through it. `routedroid doctor` reports the policy too.

## What the policy cannot allow

Even inside `phone_addresses`, the helper refuses an address that is the gateway, one of the
PC's own addresses, the network or broadcast address, already routed, or answering ARP.
