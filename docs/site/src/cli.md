# The CLI

`routedroid` talks to your daemon, `routedroidd`, over a socket in `$XDG_RUNTIME_DIR`. Every
command takes `--json` (see [Scripting](scripting.md)) and `--socket PATH`.

```sh
routedroid devices                          # phones adb sees, and whether they can be used
routedroid interfaces                       # interfaces, and whether a phone may join through each
routedroid start                            # the one phone, on the one allowed LAN; Ctrl-C disconnects
routedroid start SERIAL --lan-if eno1       # which phone and LAN, when there are several
routedroid start SERIAL --phone-ip 192.168.1.201   # a chosen address instead of a lease
routedroid status                           # connections, their address and lease, traffic
routedroid stop                             # or: routedroid stop SERIAL
routedroid events                           # follow what happens, one line each
routedroid doctor                           # check everything; --repair cleans up after a crash
routedroid version                          # the CLI's and the daemon's versions
```

`routedroid help COMMAND` or `routedroid COMMAND --help` shows every option.

The packages and `install.sh` install shell completions (bash, zsh and fish) and man pages:
`man routedroid`, and one page per command (`man routedroid-start`, ...). Elsewhere,
`routedroid completions bash|zsh|fish` prints the script for your shell.

## start

```text
routedroid start [OPTIONS] [PHONE]
```

Connects a phone, then follows the connection until it ends or you press Ctrl-C. Ctrl-C
disconnects the phone; a second Ctrl-C stops waiting and leaves the daemon to finish.

**Which phone.** `PHONE` is a serial or a [remembered name](remembered-phones.md). With one
phone attached, you can leave it out. `-s SERIAL` and the `ANDROID_SERIAL` variable work
too, as with adb.

**Which LAN.** `--lan-if` names the LAN interface the phone joins. By default it is the
remembered one, else the one interface the [policy](policy.md) allows. With several
allowed, say which.

| Option | Means |
|---|---|
| `--lan-if IF` | the LAN interface the phone joins |
| `--phone-ip ADDR` | the address the phone gets; leased by DHCP when left out |
| `--dns ADDR` | a DNS server for the phone (repeatable) |
| `--no-dns` | give the phone no DNS server at all |
| `--mtu N` | the packet MTU offered to the phone |
| `--reconnect-wait DURATION` | how long an unplugged phone keeps its connection, e.g. `5m`; `0` ends it at once |
| `--connect-timeout DURATION` | how long the app has to connect after launch, e.g. `90s` |
| `--tun NAME` | the TUN interface name (`phoneN`); a free one by default |
| `--remember` | remember this phone and these options, and connect it whenever it is plugged in |
| `--name NAME` | call the phone NAME from now on (remembers it, as `--remember` does) |
| `--detach` | return once the daemon has accepted the start |
| `--allow-network-adb` | allow a network adb serial (`host:port`); untested |

Durations read like `90s`, `2m` or `1h`.

A start the policy forbids is refused at once, with the reason, and exit code 2.

**States.** A connection goes through:

| State | Means |
|---|---|
| `starting` | the helper sets up the PC's side, and leases the address |
| `installing app` | the phone lacks the app, or has an older one; the daemon installs it |
| `waiting for app` | the app was launched and has not connected yet |
| `handshaking` | the app connected; the phone may be showing the VPN permission dialog |
| `active` | the phone is on the LAN |
| `reconnecting` | the phone went away; its address is held while it comes back |
| `stopping` | the connection is being torn down |

While waiting for the app or handshaking, the daemon checks the phone's screen every 2
seconds. A locked phone reads as `waiting for app (locked)`, and a start in the foreground
prints "the phone is locked: unlock it to continue". A phone whose screen is off reads as
`(screen off)`. Once it is unlocked, the connection goes on by itself.

## Unplugging

A phone that goes away while connected (cable out, adb restarted) is held for, as
`reconnecting`. Its address, lease, TUN and routes stay. When adb sees the phone again, the
connection resumes on its own, with the same address, and the phone's VPN comes back up.

It waits 2 minutes by default. `--reconnect-wait 10m` waits longer, and `0` ends the
connection at once. If the phone isn't back in time, the connection ends and says why. The
app closing the connection, with the phone still attached, ends it.

## stop

```text
routedroid stop [PHONE]
```

Disconnects a phone and waits until it is gone. With one connection, you can leave the phone
out. A phone stopped this way stays disconnected, even if it is remembered, until it is
plugged in again.

A connection also ends with Stop on the phone (in the app or its notification), or when the
daemon stops.

## status, devices, interfaces

- `status` lists live connections: the phone, its state, LAN, address and lease, and
  traffic.
- `devices` lists the phones adb sees, their remembered names, and whether Routedroid can
  use each.
- `interfaces` lists the PC's network interfaces and whether a phone may join through each,
  with the addresses the policy allows there.

## events

`routedroid events` follows what the daemon reports, one timestamped line each, until
Ctrl-C:

```text
14:02:11 R58M: starting
14:02:13 R58M: active
14:05:40 phones attached: none
```

With `--json`, it prints the events themselves as JSON lines.

**Desktop notifications.** On a desktop, the daemon also shows a notification when a phone
joins the LAN, goes away, waits to be unlocked, or is disconnected other than by your own
`stop`. Each phone has one notification, updated as its connection changes. To turn them
off:

```sh
systemctl --user edit routedroid     # add, under [Service]: Environment=ROUTEDROID_NOTIFY=false
systemctl --user restart routedroid
```

## doctor

`routedroid doctor` checks adb and its phones, the helper, the policy, the firewall and what
Routedroid left behind. A failing check says what to do, and doctor exits 1.
`routedroid doctor --repair` makes the changes the checks name, such as removing what a
crash left behind. Repair touches only objects that carry Routedroid's tags.

## More

- [Remembered phones](remembered-phones.md): names, and connecting on plug-in.
- [Addresses, DNS and egress](network.md): where the phone's address and traffic go.
- [Exit codes](exit-codes.md).
