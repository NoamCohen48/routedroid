# Set up

After installing, run once:

```sh
sudo routedroid setup
```

It does what a first connection needs from root, and asks before it chooses anything:

- **Your group.** It adds you to group `routedroid`, whose members may ask the helper to
  connect phones. `--user NAME` sets someone else up; by default it is the user who ran
  `sudo`.
- **The LAN.** It lets phones join the LAN through one interface, in
  `/etc/routedroid/helper.toml`. It offers the interface with the default route. Phones
  either lease an address there by DHCP, or take one from a block of addresses you name.
  The old file is kept as `helper.toml.bak`, and the helper confirms the new one.
- **Your daemon.** It enables `routedroid.service`, a user unit.

It ends with what is left to do. Usually that is logging out completely, since a new group
reaches only new sessions, and then `routedroid start`.

Run it again any time. It changes only what isn't set up already.

## Without a terminal

Choose with flags instead of answering questions:

| Flag | Means |
|---|---|
| `--lan-if eno1` | the LAN interface phones join through |
| `--dhcp` | phones lease their address from the LAN's DHCP server (the default unless `--phone-addresses` is given) |
| `--phone-addresses 192.168.1.200/29` | a block of addresses phones may take (repeatable) |
| `-y`, `--yes` | take the default answers |
| `--user NAME` | who will connect phones |

For example:

```sh
sudo routedroid setup --lan-if eno1 --phone-addresses 192.168.1.200/29 --yes
```

## By hand

The same steps, without `setup`:

```sh
sudo usermod -aG routedroid "$USER"          # then log out completely
systemctl --user enable --now routedroid
routedroid interfaces                        # which interfaces exist, and what the policy allows
sudoedit /etc/routedroid/helper.toml         # allow one; see "The helper policy"
```

See [The helper policy](policy.md) for the file's format.

## Check it

```sh
routedroid doctor
```

It reports adb and its phones, the helper, which interfaces the policy opens, and anything a
crash left behind. A failing check comes with what to do, and doctor exits 1.

Next: [Quick start](quick-start.md).
