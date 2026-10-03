# Routedroid

Routedroid makes an Android phone plugged into a Linux PC a real host on the PC's LAN. The
phone gets its own IPv4 address there, leased from the LAN's DHCP server or chosen by you.
Other machines on the LAN can reach it, and its traffic leaves through that LAN. Nothing on
the phone is rooted, and nothing on the LAN changes.

Packets travel as raw IPv4 over `adb reverse` into an Android `VpnService`. On the PC a TUN
device, a `/32` route and proxy ARP on the LAN interface make the address answer.

```
LAN host ── LAN ── eno1 (proxy ARP) ── phone0 (TUN) ── adb ── VpnService ── apps on the phone
```

## Requirements

- **PC:**
  - Linux with nftables, policy routing and TUN (any current distribution);
  - systemd;
  - `adb` (Android platform-tools), with USB access to the phone (your distribution's
    `android-udev-rules`, or membership in `plugdev`);
  - Rust (see `host/rust-toolchain.toml`) to build.
- **Phone:**
  - Android 8.0 or newer;
  - USB debugging enabled and this PC authorized;
  - the Routedroid app (`android/`, built with Gradle and JDK 17).
- **LAN:**
  - a wired or Wi-Fi network you may add a host to;
  - for DHCP, a server that leases one more address to the PC's MAC under another client
    identifier (most home and office routers do).

## Install

```sh
cd host && cargo build --release && sudo ./install.sh     # then log in again (group routedroid)
systemctl --user enable --now routedroid                  # the daemon, as yourself
(cd android && ./gradlew :app:assembleDebug)              # release builds: android/README.md
adb install android/app/build/outputs/apk/debug/app-debug.apk
```

The install puts these in place:

- `routedroid`, `routedroidd` and `routedroid-tui` in `/usr/local/bin`;
- the root helper in `/usr/local/libexec/routedroid`;
- the helper's socket-activated units and the daemon's user unit;
- group `routedroid`, with you added to it;
- a policy that allows nothing.

Next, allow the LAN interface the phones may join. `routedroid interfaces` lists the
candidates. Then edit `/etc/routedroid/helper.toml`:

```toml
[[interface]]
name = "eno1"
dhcp = true                                  # phones may lease an address here
phone_addresses = ["192.168.1.200/29"]       # and may ask for one of these (bounds leases too)
```

The file must stay root-owned and not writable by group or others, or the helper refuses
every session. An interface missing from the file, or one with neither key, admits no phone.

Check the setup with `routedroid doctor`. It reports:

- adb and its phones;
- the helper;
- which interfaces the policy opens;
- anything a crash left behind.

## Use

```sh
routedroid devices                          # phones adb sees, and whether they can be used
routedroid start -s SERIAL --lan-if eno1    # leases an address; Ctrl-C disconnects
routedroid start -s SERIAL --lan-if eno1 --phone-ip 192.168.1.201   # a chosen address
routedroid status                           # connections, their address and lease, traffic
routedroid stop -s SERIAL
routedroid-tui                              # the same, interactively
```

The first time, the phone asks for VPN permission. Answer it on the phone. While connected,
the phone shows its VPN key icon and a notification with Stop. A connection belongs to the
daemon, not to the command that started it. `start --detach` returns immediately, and the
phone stays on the LAN until `routedroid stop`, Stop on the phone, unplugging, or the daemon
stopping. Several phones can be connected at once, each with its own address.

Every command takes `--json`. `routedroid events` streams state changes as JSON lines. Exit
codes are listed in `routedroid --help`, so scripts can branch on why something failed.

**Addresses.** Without `--phone-ip` the helper leases an address on the LAN interface. The
client identifier is `routedroid:<device id>:<PC MAC>`, so the same phone tends to get the same
address back. The helper renews the lease for as long as the connection lasts and releases
it at the end, even after a crash. If the lease is lost, or another station claims the
address, the connection ends with the reason. With `--phone-ip` the address must lie in
`phone_addresses`. The helper refuses it if it answers ARP, is the gateway, is one of the
PC's own addresses, or is already routed.

**DNS.** By default the phone uses the lease's DNS servers, or else the LAN's gateway.
`--dns` and `--no-dns` override this.

**Egress.** The phone's traffic always leaves through its LAN: through the lease's router,
else through the LAN interface's own default route. This holds even when the PC's default
route goes elsewhere (a VPN, another uplink). With no gateway, the phone reaches that LAN and
nothing else.

## What runs with privileges

| Part | Runs as | Can do |
|---|---|---|
| `routedroidd` | you (`systemctl --user`) | adb, the packet relay, the control socket in `$XDG_RUNTIME_DIR` |
| `routedroid-helper` | root, one instance per connection (socket activation) | only what `/etc/routedroid/helper.toml` allows, for members of group `routedroid` |

The helper takes typed requests only. A start names a LAN interface, a TUN name (`phoneN`),
and optionally an address. The helper derives everything else, and the policy check comes
before any kernel change or packet. Each change is written to a journal on disk before it is
made, and undone in reverse order when the connection ends. If a helper instance dies, even by
SIGKILL, systemd runs `routedroid-helper cleanup`, which replays the journal. Undo removes
only objects tagged with that session's id. A shared sysctl (`forwarding`, `proxy_arp`) is
restored only when the last phone on that interface leaves, and only if nobody changed it
meanwhile. Details are in [the helper's README](host/routedroid-helper/README.md) and
[the architecture](.docs/architecture.md).

The phone accepts a session only from the PC that wrote its one-time secret over adb, and only
after you grant VPN permission. A phone holds one connection at a time. It cannot reach the
PC's other interfaces, other phones, or addresses it was not given.

## Firewalls

Routedroid adds one nftables table per connection (`inet routedroid_phoneN`), with
policy-accept chains that filter only that phone's traffic. An accept there cannot override a
drop in your own firewall. If `routedroid doctor` warns about a forward chain that drops by
default (firewalld, UFW and many hand-written rulesets do), let the phone interfaces through.
For example:

```sh
nft insert rule inet filter forward iifname "phone*" accept        # your own nftables table
nft insert rule inet filter forward oifname "phone*" accept
ufw route allow in on phone0; ufw route allow out on phone0         # UFW, per TUN name
```

With firewalld, put each TUN in a zone that forwards to and from your LAN's zone (for example
`firewall-cmd --zone=trusted --add-interface=phone0`), then check from another LAN host.

Routedroid's own table still limits each phone to its address and its LAN.

## Troubleshooting

- **`routedroid doctor`** comes first. It reports a failing check with what to do, and exits 1.
- **The daemon is unreachable** (exit 3): `systemctl --user start routedroid`, and check
  `journalctl --user -u routedroid`.
- **Helper refused or unreachable**: check that `routedroid-helper.socket` is active and that
  you are in group `routedroid` (in a new login). The helper logs to
  `journalctl -u 'routedroid-helper@*'`.
- **No lease** (`NoLease`): the LAN has no DHCP server or does not answer this client. Use
  `--phone-ip` with an address from `phone_addresses`.
- **"is the Routedroid app installed?"**: run `adb install` with the APK.
- **Waiting for the app**: answer the VPN dialog on the phone. `--connect-timeout` gives more
  time.
- **Connected, but the phone gets no traffic**: look for a firewall warning in
  `routedroid doctor` (see Firewalls above). Also make sure the LAN is not isolating clients.
- **After a crash or a power loss**: `routedroid doctor` lists what was left behind, and
  `routedroid doctor --repair` removes it. Repair touches only objects that carry
  Routedroid's tags.

## Uninstall

```sh
sudo host/install.sh --uninstall            # replays journals, removes leftovers, binaries and units
sudo host/install.sh --uninstall --purge    # also the policy, /var/lib/routedroid and the group
adb uninstall dev.routedroid
```

Stop the daemon first (`systemctl --user disable --now routedroid`). Afterwards no TUN, route,
rule, nftables table or sysctl change of Routedroid's remains. `--purge` keeps the state
directory if anything could not be undone.

## Known limits

- IPv4 only: the phone has no IPv6 while connected.
- One LAN per phone. Android picks one source address, so a second LAN could not be answered
  correctly.
- Not an Ethernet bridge: no broadcast or multicast discovery (mDNS, SSDP) reaches the phone.
- Networks that allow one MAC per port to hold one address (strict DHCP snooping, 802.1X with
  a single host, some captive portals), and networks that block proxy ARP, do not work.
  `--phone-ip` does not help there either.
- The phone shows that a VPN is active, and another VPN app on it takes over the connection.
- Network ADB (`host:port` serials) needs `--allow-network-adb` and is not verified. The VPN's
  default route may cut adb itself.

## Repository

| Path | What |
|---|---|
| `host/` | Rust workspace: daemon, CLI, TUI, root helper, DHCP client, wire protocol, IPC crates, fuzz targets |
| `android/` | The app (`VpnService`), its protocol library and a hostile test app ([README](android/README.md)) |
| `protocol/` | The wire protocol ([version 1](protocol/version-1.md)) and its golden fixtures |
| `integration-tests/` | Rootless namespace rigs for the helper (kill matrix, multi-session, DHCP) and emulator rigs |
| `.docs/` | Architecture, decisions, implementation plan, code review |

Tests: run `cargo test --workspace` in `host/`, `./gradlew :protocol:test :app:testDebugUnitTest`
in `android/`, and the rigs in `integration-tests/helper/`, which need no root. CI runs them
all (`.github/workflows/ci.yml`).
