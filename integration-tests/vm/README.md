# VM lab: real root, real systemd, a real phone

The namespace rigs in `../helper/` need no root, but they never run `install.sh`, the systemd
units, the policy file or a full login. This lab does, unprivileged: KVM guests on a virtual
LAN, with a USB phone passed into the guest that plays the PC.

```
router (dnsmasq DHCP/DNS, NAT, LAN peer)  ── lan0 192.168.80.0/24 ──  host (Routedroid) ── USB ── phone
      └ mgmt0: qemu user net, the Internet                                └ mgmt0: ssh from here
```

| Guest | Memory | ssh | Role |
|---|---|---|---|
| `router` | 384 MB | `127.0.0.1:2201` | `192.168.80.1`: DHCP (leases `.100`–`.150`, 10 min), DNS, NAT out through mgmt0, and the "other machine on the LAN" |
| `host` | 1 GB | `127.0.0.1:2202` | Debian 13 with Routedroid installed by `install.sh`; lan0 by DHCP and the default route, mgmt0 a second default (metric 1000) |
| `ubuntu` | 1 GB | `127.0.0.1:2203` | The same PC as Ubuntu 24.04 with ufw enabled (routed traffic dropped, as shipped) |

The guests are Debian 13 and Ubuntu 24.04 cloud images with cloud-init (`router.yaml`, `host.yaml`,
`network.yaml`). Guests are copy-on-write overlays in `~/.cache/routedroid-vm`, so
`./lab.sh destroy NAME` followed by `up` rebuilds one from scratch in about a minute.

## Needs

- `/dev/kvm` readable and writable by you, plus `qemu-system-x86_64`, `qemu-img`, `mkfs.vfat`
  and `mcopy` (dosfstools, mtools);
- the base images, once:
  ```sh
  mkdir -p ~/.cache/routedroid-vm && cd ~/.cache/routedroid-vm
  curl -fLO https://cloud.debian.org/images/cloud/trixie/latest/debian-13-generic-amd64.qcow2
  curl -fLO https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img
  ```
- for a phone: write access to its USB node (the adb udev rules give it), and its adb key in
  the guest (below).

## Run

```sh
./lab.sh up                               # router and host; first boot installs packages
./push.sh                                 # build here, install in host with install.sh
./lab.sh ssh host                         # then: systemctl --user enable --now routedroid
```

For a phone, free it on this PC and hand it to the guest:

```sh
adb kill-server                                       # this PC's adb lets go of it
systemctl --user stop gvfs-mtp-volume-monitor         # GNOME: or gvfsd-mtp grabs it again on any re-plug
tar -C ~ -czf - .android/adbkey .android/adbkey.pub | ./lab.sh ssh host 'tar -C ~ -xzf -'   # no "Allow USB debugging?" prompt
./lab.sh down host && PHONE=04e8:6860 ./lab.sh up host   # vendor:product from lsusb
./phone-session.sh SERIAL                             # GUEST=ubuntu for the ufw PC
```

The phone can be pulled and put back while a guest runs (`PHONE=… ./lab.sh unplug|plug NAME`,
through qemu's QMP socket), as if the cable came out.

The rigs run `../emulator/prepare-device.sh`, whose `appops set … ACTIVATE_VPN allow` does
not take on Samsung. After the app is (re)installed, answer its VPN dialog once on the phone.
Consent then persists across connections.

Afterwards, `./lab.sh down` and `systemctl --user start gvfs-mtp-volume-monitor`. The phone
returns to this PC once qemu exits.

## phone-session.sh

These are the checks:

- **Setup:** doctor passes. The start is leased, and dnsmasq holds the lease under
  Routedroid's client-id. The phone's egress rule and table are in place.
- **From the LAN:** the router pings the phone and opens a TCP connection to it.
- **From the phone:** it reaches the router and the Internet, and resolves names. A capture
  on the LAN shows its own address, so the host does no NAT.
- **Stop:** the lease is RELEASEd, and no rule, table or leftover remains.

17 checks.

## ufw.sh

`ufw.sh SERIAL` runs on the `ubuntu` guest:

- **With ufw's defaults:** doctor warns about ufw's `FORWARD` chain and gives the ufw
  command, with no IPv6 noise. Ping reaches the phone, but TCP does not.
- **After running doctor's command as printed:** the warning is gone, and
  `phone-session.sh` passes.

8 checks.

## package.sh

`package.sh SERIAL` takes the `ubuntu` guest through the .deb from `host/packaging/build.sh`,
from a clean guest:

- **Install:** apt installs it. The message's steps are followed: join the group, then
  restart the user manager. The daemon then runs from `/usr/bin`, and `phone-session.sh`
  passes.
- **Upgrade under a live session:** needrestart defers the helper instance instead of
  restarting it, and the session stays up and keeps carrying traffic.
- **Remove under a live session:** nothing is left behind, and the policy and group remain.
- **Purge:** no policy, state or group remains.

11 checks. `../packages/containers.sh` covers the same life without a phone in Debian, Ubuntu
and Fedora containers, and CI runs it.

## unplug.sh

`PHONE=vendor:product unplug.sh SERIAL` runs on the `ubuntu` guest. qemu removes the phone's
USB device from the guest and adds it back:

- **Unplugged and back:** while away, the connection is `reconnecting`, and the address,
  egress rule and TUN are held. Back, it is active again with the same address, on the same
  helper session, reachable from the LAN and reaching it.
- **The app closes it, the phone stays:** it ends as "the phone closed the connection".
- **Away past `--reconnect-wait 15s`:** it ends and says why. The lease is RELEASEd, nothing
  is left, and the phone is back for the next connection.

20 checks.

## app-install.sh

`app-install.sh SERIAL` runs on the `ubuntu` guest, with the .deb built with `ROUTEDROID_APK`:

- **A phone without the app:** the connection installs the daemon's app first. The phone
  asks for VPN permission, which the rig gives through the UI. The connection goes active.
- **The app is current:** the next connection installs nothing.

10 checks. The phone is left with that app. Put a debug build back by uninstalling it and
running `adb install`.

## Results, 2026-10-06 (Samsung SM-J810G, Android 10)

| Check | Result |
|---|---|
| `phone-session.sh 85e49002` (Debian, nftables) | 17/17 |
| `ufw.sh 85e49002` (Ubuntu 24.04, ufw) | 8/8, with `phone-session.sh` 17/17 inside |
| `package.sh 85e49002` (the .deb) | 11/11 |
| `PHONE=04e8:6860 unplug.sh 85e49002` | 20/20; back to active about 3 s after the replug |
| `phone-session.sh` with the release APK (R8, signed with a test key) | 17/17 |
| `app-install.sh 85e49002` (2026-10-07, the daemon carrying that APK) | 10/10 |
| Phone dropped off USB mid-session (gvfs re-grab) | The connection ended ("the phone closed the connection"), the lease was RELEASEd, and no rule or table was left |
| First .deb upgrade | Found that needrestart restarts a helper instance, which ends its session; the .deb now tells needrestart not to |
| First install on a fresh guest | Found that a lingering `systemd --user` keeps its old groups; `doctor` now says so (`089a27d`) |
