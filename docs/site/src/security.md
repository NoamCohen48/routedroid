# Security model

## What runs with privileges

| Part | Runs as | Can do |
|---|---|---|
| `routedroidd` | you (`systemctl --user`) | adb, the packet relay, the control socket in `$XDG_RUNTIME_DIR` |
| `routedroid-helper` | root, one instance per connection (socket activation) | only what `/etc/routedroid/helper.toml` allows, for members of group `routedroid` |

Only the helper runs as root, and only while a connection lasts. Anyone on the PC may connect to
its socket, but before it reads a request the helper takes the caller's uid from the kernel
(`SO_PEERCRED`) and turns away anyone but root and the members of group `routedroid`. It asks
the group database each time rather than the caller's login groups, so a refusal says why and
a member who joined a moment ago is admitted. A lookup that fails or takes over 5 seconds
refuses.

The lookup runs inside the helper's sandbox, which may open no IP socket. Local files work,
and so do directories reached through a local service: SSSD, nslcd, winbind, systemd-userdbd.
An NSS module that calls a directory server over the network itself, such as the old
`libnss-ldap`, cannot, so its users are refused. Use SSSD or nslcd instead.

## The helper

- **Typed requests only.** A start names a LAN interface, a TUN name (`phoneN`) and
  optionally an address. The helper derives everything else itself, and validates every
  name.
- **Policy first.** The [policy](policy.md) is checked before any change to the kernel and
  before any packet is sent. A missing, unsafe or unparseable policy refuses everything.
- **Addresses are checked.** Even inside the policy, the helper refuses gateways, the PC's
  own addresses, the network and broadcast addresses, already-routed addresses, and an
  address that answers ARP.
- **One instance per connection.** Each connection gets its own helper process, so one
  instance's crash never touches another's session.
- **Journaled.** Every change is written to disk before it is made, and undone in reverse
  order when the connection ends, even after SIGKILL.
- **Tagged.** The TUN, the nftables table and the routes carry the session's tag. Undo and
  `doctor --repair` remove only objects that carry Routedroid's tags, never a same-named
  stranger.
- **Confined.** The helper's systemd unit allows only what it needs: kernel tunables are
  read-only except the per-interface ones it changes, there is a syscall filter, and it may
  open no IP sockets (DHCP goes through a packet socket).

## The phone

- The phone accepts a session only from the PC that wrote its one-time secret over adb.
  Both sides prove they know it, so another app on the phone or the PC cannot take the
  session over.
- Nothing starts without your VPN permission on the phone.
- A phone holds one connection at a time.

## The phone on the LAN

Each phone's nftables table limits it to its own address and its LAN. It cannot reach the
PC's other interfaces, other phones, or addresses it was not given. Its traffic leaves only
through its LAN.

The phone becomes a host on your LAN like any other. Whatever the LAN can reach, it can
reach; whatever can reach the LAN can reach it. Treat it as you would a laptop joining that
network.
