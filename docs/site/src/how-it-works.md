# How it works

```text
LAN host ── LAN ── eno1 (proxy ARP) ── phone0 (TUN) ── adb ── VpnService ── apps on the phone
```

## The parts

| Part | Runs as | Does |
|---|---|---|
| `routedroid`, `routedroid-tui` | you | ask the daemon, and show what it says |
| `routedroidd` | you, as a user service (`systemctl --user`) | talks to adb, carries the packets, owns the connections |
| `routedroid-helper` | root, one instance per connection | changes the PC's network, as the policy allows |
| the app | the phone | a `VpnService` that hands the phone's packets to the PC |

## A connection, step by step

0. **The app, if needed.** A phone without the app, or with an older one, gets the app the
   daemon carries before anything else is set up.
1. **The helper sets up the PC's side.** The daemon asks the root helper for a session on
   the LAN interface. The helper checks the [policy](policy.md), leases or checks the
   address, and then:
   - creates the TUN device (`phone0`);
   - adds a `/32` route to the phone's address through it;
   - turns on proxy ARP and forwarding on the LAN interface, so the LAN finds the phone's
     address at the PC;
   - adds an nftables table that limits the phone to its address and its LAN;
   - adds a routing rule and table so the phone's traffic leaves through that LAN.
2. **The daemon reaches the phone.** It sets up `adb reverse` to a port on the PC's
   loopback, writes a one-time secret to the app over adb, and launches it.
3. **The app dials in.** It connects back through `adb reverse`, and both sides prove they
   know the secret.
4. **The phone sets up its VPN.** The app asks for VPN permission the first time. It gets
   its address, DNS and MTU from the PC, and brings the VPN up.
5. **Packets flow.** Every IPv4 packet on the phone goes through the VPN, over adb, to the
   daemon, into the TUN, and out on the LAN. Replies take the same way back.

When the connection ends, the helper undoes every change, in reverse order, and releases the
lease.

## Unplugging

When the phone goes away, the daemon keeps the helper's session: the address, lease, TUN and
routes stay, and the connection is `reconnecting`. When adb sees the phone again, the daemon
runs steps 2 to 4 again, and the phone is back with the same address.

## Crashes

Each change the helper makes is written to a journal on disk before it is made. If a helper
instance dies, even by SIGKILL, systemd runs `routedroid-helper cleanup`, which replays the
journal and undoes what was done. If the daemon dies, its socket to the helper closes, and the
helper undoes everything and exits.

Undo removes only objects tagged with that session's id. A shared setting such as
`forwarding` or `proxy_arp` is restored only when the last phone on that interface leaves,
and only if nobody changed it meanwhile.

## Further reading

In the repository:

- `.docs/architecture.md`: the architecture in full;
- `host/routedroid-helper/README.md`: the helper, its journal and its checks;
- `protocol/version-1.md`: the wire protocol between the daemon and the app;
- `android/README.md`: the app, and how a session starts on the phone.
