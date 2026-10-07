# Troubleshooting

Start with:

```sh
routedroid doctor
```

It checks adb and its phones, the helper, the policy, the firewall, and what Routedroid
left behind. A failing check says what to do, and doctor exits 1.

## The daemon is unreachable (exit 3)

```sh
systemctl --user start routedroid
journalctl --user -u routedroid
```

## The helper refused, or is unreachable

Check that `routedroid-helper.socket` is active, and that the daemon has group `routedroid`.
`doctor` tells the two cases apart.

A `systemd --user` that started before you joined the group keeps running without it until
you log out completely, or until `sudo systemctl restart user@$(id -u)`.

The helper logs to `journalctl -u 'routedroid-helper@*'`.

## "not in the helper policy"

The interface isn't allowed in `/etc/routedroid/helper.toml`. Run `sudo routedroid setup`, or
see [The helper policy](policy.md). An address outside `phone_addresses` is refused the same
way.

## No lease

The LAN has no DHCP server, or it doesn't answer this client. Use `--phone-ip` with an
address from `phone_addresses`.

## "is the Routedroid app installed?"

This daemon carries no app (a source build). Install it with `adb install` and the APK.

## "could not install the Routedroid app"

The phone refused the daemon's app, and the reason follows the message.
`INSTALL_FAILED_UPDATE_INCOMPATIBLE` means another build of the app, signed with another
key, is installed. Uninstall it with `adb uninstall dev.routedroid`.

## Waiting for the app

The app was launched but hasn't connected. Unlock the phone, and check that the app is
installed. `--connect-timeout` gives it more time.

<!-- TODO: the locked-phone hint, once it lands. -->

## Stuck at handshaking

The phone is showing the VPN permission dialog. Unlock the phone and answer it. Without an
answer within 2 minutes, the connection ends.

## Connected, but the phone gets no traffic

Look for a firewall warning in `routedroid doctor` (see [Firewalls](firewalls.md)). Also
make sure the LAN isn't isolating clients, as some guest Wi-Fi networks do.

## Watching packets

```sh
tcpdump -ni phone0                        # what the phone sends and receives
tcpdump -ni eno1 host 192.168.1.57        # the same packets on the LAN
ip rule; ip route show table all          # the phone's egress
```

The phone's own address appears on both, with no NAT.

## After a crash or a power loss

`routedroid doctor` lists what was left behind, and `routedroid doctor --repair` removes it.
Repair touches only objects that carry Routedroid's tags. Usually there is nothing to do:
the helper undoes a dead connection by itself.

## Logs

| What | Where |
|---|---|
| the daemon | `journalctl --user -u routedroid` |
| the helper | `journalctl -u 'routedroid-helper@*'` |
| the app | `adb logcat -s DeviceLink PacketPath BootstrapProvider` |
