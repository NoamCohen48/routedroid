# routedroidd

Headless daemon that owns *device connections* — one per phone that is on the LAN — while
`routedroid` (the CLI) and `routedroid-tui` are its clients over a Unix socket. A device
connection outlives the client that asked for it: start one and quit, and the phone stays
reachable until someone stops it or the daemon shuts down.

Built from three components with no object above them (decision 0006): `AttachedDevices`
mirrors what adb reports, `DeviceConnections` owns the phones we have put on the LAN, and
`EventBus` carries what clients are told. A client connection holds those three and nothing
else; `Daemon` only builds them and stops them.

Install with the helper, as a `systemd --user` service:

1. `cargo build --release` in `host/`, then `sudo host/install.sh` (binaries in
   `/usr/local/bin`, this unit in `/etc/systemd/user`, the helper's units system-wide).
2. As yourself: `systemctl --user enable --now routedroid`.
3. Check with `routedroid version`, `routedroid devices` and `routedroid interfaces`.

The unit is sandboxed only as far as a user unit can be without privileges, and leaves the
filesystem alone: an adb server the daemon starts inherits the unit and needs `~/.android`
and the USB devices.
