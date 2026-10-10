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

**Documentation:** <https://noamcohen48.github.io/routedroid/> (built from `docs/site/`).

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

From packages (a [release](https://github.com/NoamCohen48/routedroid/releases), or
`host/packaging/build.sh`, which writes them to `host/target/packages`):

```sh
sudo apt install ./routedroid_0.1.0_amd64.deb      # Debian, Ubuntu
sudo dnf install ./routedroid-0.1.0-1.x86_64.rpm   # Fedora
sudo routedroid setup                              # see "Set up" below
```

Debian and Ubuntu can instead take it, and every update after, from the signed apt
repository on the docs site; [Install](https://noamcohen48.github.io/routedroid/install.html)
has the three lines that add it.

On another systemd distribution (glibc 2.35 or newer, with nftables and adb installed), the
release's tarball holds the same programs and `install.sh`, which installs into `/usr/local`:

```sh
tar -xzf routedroid-0.1.0-linux-x86_64.tar.gz
sudo routedroid-0.1.0-linux-x86_64/install.sh
sudo /usr/local/bin/routedroid setup
```

A release's daemon carries the app. When a phone connects without it, or with an older
version, the daemon installs the app first (state `installing app`). The phone then asks
once for VPN permission. The release's APK is there too, for installing by hand.

Or from source, into `/usr/local`:

```sh
cd host && cargo build --release && sudo ./install.sh && sudo /usr/local/bin/routedroid setup
(cd android && ./gradlew :app:assembleDebug)              # release builds: android/README.md
adb install android/app/build/outputs/apk/debug/app-debug.apk
```

A daemon built from source carries no app unless it is built with `ROUTEDROID_APK` set to a
signed APK's absolute path (`host/packaging/build.sh` passes it through).

Either way, these get installed:

- `routedroid`, `routedroidd` and `routedroid-tui`, in `/usr/bin` (from the packages) or
  `/usr/local/bin`;
- the root helper, in `libexec/routedroid` under the same prefix;
- the helper's socket-activated units, enabled, and the daemon's user unit;
- group `routedroid`;
- a policy that allows nothing.

## Set up

`sudo routedroid setup` does what a first connection needs from root, and asks before
choosing:

- it adds you to group `routedroid`, which may ask the helper for phones;
- it lets phones join the LAN through one interface, in `/etc/routedroid/helper.toml`. It
  offers the one with the default route, by DHCP, or a block of addresses you name. The old
  file is kept as `helper.toml.bak`, and the helper confirms the new one;
- it enables your daemon (`routedroid.service`, a user unit).

It ends with what is left, usually just `routedroid start`. No logout is needed: the helper
looks the group up each time the daemon connects, so a group joined just now counts. Run it
again any time; it changes only what is not so already. You can also skip it: a first
`routedroid start` at a terminal sees what is missing and offers to run it, then goes on with
the start. Without a terminal, choose with flags: `--lan-if eno1` (DHCP by default),
`--phone-addresses 192.168.1.200/29`, or `--yes` for the defaults.

By hand instead: `sudo usermod -aG routedroid "$USER"`,
`systemctl --user enable --now routedroid`, and allow an interface from
`routedroid interfaces` in `/etc/routedroid/helper.toml`:

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
routedroid start                            # the one phone, on the one allowed LAN; Ctrl-C disconnects
routedroid start SERIAL --lan-if eno1       # which phone and LAN, when there are several
routedroid start SERIAL --phone-ip 192.168.1.201   # a chosen address instead of a lease
routedroid start SERIAL --name pixel --remember    # and connect it whenever it is plugged in
routedroid status                           # connections, their address and lease, traffic
routedroid stop                             # or: routedroid stop pixel
routedroid-tui                              # the same, interactively
```

The first time, the phone asks for VPN permission. Answer it on the phone. While connected,
the phone shows its VPN key icon and a notification with Stop. A connection belongs to the
daemon, not to the command that started it. `start --detach` returns immediately, and the
phone stays on the LAN until `routedroid stop`, Stop on the phone, or the daemon stopping.
Several phones can be connected at once, each with its own address.

**Which phone, which LAN.** With one phone attached, `start` takes it; with several, name
one (its serial, or `-s`, or `ANDROID_SERIAL`). With one interface allowed in the helper
policy, `start` uses it; with several, say which with `--lan-if`.

**Remembered phones.** `--remember` keeps the phone, with the options given, in
`~/.config/routedroid/phones.toml`, and the daemon connects it whenever it is plugged in
and authorized. `--name pixel` gives it a name that every command takes in place of the
serial. `routedroid phones` lists them, `routedroid remember PHONE` changes one
(`--no-auto`: remember the options without connecting on plug-in), and `routedroid forget
PHONE` drops one. A phone disconnected with `stop` stays disconnected until it is plugged in
again. In the TUI, the form's Name and Remember fields do the same, and `f` forgets.

**Unplugging.** A phone that goes away while connected (cable out, adb restarted) is held
for it, as `reconnecting`: its address, lease, TUN and routes stay. When adb sees the phone
again, the connection resumes on its own, with the same address, and the phone's VPN comes
back up. It waits 2 minutes by default. `--reconnect-wait 10m` waits longer, and `0` ends
the connection at once. The app closing the connection, with the phone still attached, ends
it.

**Notifications.** On a desktop, the daemon shows a notification when a phone joins the LAN,
goes away, waits to be unlocked, or is disconnected other than by your own `stop`. Each phone
has one notification, updated as its connection changes. `routedroid notifications off`
turns them off, and `on` turns them back on; the daemon keeps the choice.

Every command takes `--json`. `routedroid events` follows state changes as they happen, one
line each (with `--json`, as JSON lines). Exit codes are listed in `routedroid --help`, so
scripts can branch on why something failed.

The packages and `install.sh` install shell completions (bash, zsh, fish) and man pages
(`man routedroid`, `man routedroid-start`, ...). Elsewhere, `routedroid completions SHELL`
prints the script for your shell.

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
drop in your own firewall. ufw, as Ubuntu ships it, is the common case. Ping still reaches the
phone, but TCP and UDP to and from it are dropped, DNS included.

`routedroid doctor` finds IPv4 forward chains that drop by default and prints the command
that lets the phone interfaces through, in the firewall's own terms:

```sh
sudo ufw route allow in on phone+ && sudo ufw route allow out on phone+            # ufw (tested)
sudo iptables -I FORWARD -i phone+ -j ACCEPT && sudo iptables -I FORWARD -o phone+ -j ACCEPT
sudo nft insert rule inet filter forward iifname "phone*" accept && \
    sudo nft insert rule inet filter forward oifname "phone*" accept                # your own table
```

Once the table lets `phone*` through both ways, doctor stops warning. With firewalld, put each
TUN in a zone that forwards to and from your LAN's zone (for example
`firewall-cmd --zone=trusted --add-interface=phone0`), then check from another LAN host.

Routedroid's own table still limits each phone to its address and its LAN.

## Troubleshooting

- **`routedroid doctor`** comes first. It reports a failing check with what to do, and exits 1.
- **The daemon is unreachable** (exit 3): `systemctl --user start routedroid`, and check
  `journalctl --user -u routedroid`.
- **Helper refused or unreachable**: check that `routedroid-helper.socket` is active and that
  you are in group `routedroid` (`sudo routedroid setup` adds you). `doctor` tells the two
  cases apart. The helper logs to `journalctl -u 'routedroid-helper@*'`.
- **No lease** (`NoLease`): the LAN has no DHCP server or does not answer this client. Use
  `--phone-ip` with an address from `phone_addresses`.
- **"is the Routedroid app installed?"**: this daemon carries no app (a source build). Run
  `adb install` with the APK.
- **"could not install the Routedroid app"**: the phone refused the daemon's app. Its reason
  follows the message. `INSTALL_FAILED_UPDATE_INCOMPATIBLE` means another build of the app,
  signed with another key, is installed. Uninstall that one (`adb uninstall dev.routedroid`).
- **"the phone is locked: unlock it to continue"**: the daemon checks the phone's screen
  while it waits on the phone, because the app does not start behind the lock screen and the
  VPN dialog cannot be answered there. Unlock it and the connection goes on by itself.
- **Waiting for the app**: the app was launched but has not connected. Check that the app is
  installed. `--connect-timeout` gives more time.
- **Stuck at handshaking**: the phone is showing the VPN permission dialog. Unlock the phone
  and answer it. Without an answer within 2 minutes, the connection ends.
- **Connected, but the phone gets no traffic**: look for a firewall warning in
  `routedroid doctor` (see Firewalls above). Also make sure the LAN is not isolating clients.
- **Watching packets**: `tcpdump -ni phone0` shows what the phone sends and receives, and
  `tcpdump -ni eno1 host <phone address>` shows the same packets on the LAN. The phone's own
  address appears on both, with no NAT. `ip rule` and `ip route show table all` show its egress.
- **After a crash or a power loss**: `routedroid doctor` lists what was left behind, and
  `routedroid doctor --repair` removes it. Repair touches only objects that carry
  Routedroid's tags.

## Uninstall

```sh
sudo apt remove routedroid    # or dnf remove; apt purge also drops the policy, state and group
sudo host/install.sh --uninstall            # a source install
sudo host/install.sh --uninstall --purge    # also the policy, /var/lib/routedroid and the group
adb uninstall dev.routedroid
```

Removal ends every connection first: it replays the journals and removes what was left behind,
even with phones connected. Afterwards no TUN, route, rule, nftables table or sysctl change of
Routedroid's remains. A purge keeps the state directory, and the group, if anything could not
be undone. The daemon keeps running until `systemctl --user disable --now routedroid`.

An upgrade leaves connections up. Running ones keep their helper, and the next one starts the
new version.

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
| `host/` | Rust workspace: daemon, CLI, TUI, root helper, DHCP client, wire protocol, IPC crates, fuzz targets; `install.sh` and `packaging/` (.deb, .rpm) |
| `android/` | The app (`VpnService`), its protocol library and a hostile test app ([README](android/README.md)) |
| `protocol/` | The wire protocol ([version 1](protocol/version-1.md)) and its golden fixtures |
| `integration-tests/` | Rootless namespace rigs for the helper (kill matrix, multi-session, DHCP), emulator rigs, and a [KVM lab](integration-tests/vm/README.md) with real root, systemd and a USB phone |
| `docs/site/` | The [documentation site](https://noamcohen48.github.io/routedroid/), an mdBook (`mdbook serve docs/site`) |
| `.docs/` | Architecture, decisions, implementation plan, code review |

Tests: run `cargo test --workspace` in `host/`, `./gradlew :protocol:test :app:testDebugUnitTest`
in `android/`, and the rigs in `integration-tests/helper/`, which need no root. CI runs them
all, and builds the packages and installs, upgrades and removes them in Debian, Ubuntu and
Fedora containers (`.github/workflows/ci.yml`). A `vX.Y.Z` tag drafts a release with the
packages and the APK (`.github/workflows/release.yml`).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option. Unless you explicitly state otherwise, any
contribution intentionally submitted for inclusion in Routedroid, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or conditions.
