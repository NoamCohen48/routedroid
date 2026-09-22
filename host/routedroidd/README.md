# routedroidd

Headless daemon that owns *device connections* — one per phone that is on the LAN — while
`routedroid` (the CLI) and `routedroid-tui` are its clients over a Unix socket. A device
connection outlives the client that asked for it: start one and quit, and the phone stays
reachable until someone stops it or the daemon shuts down.

Install as a `systemd --user` service:

1. `cargo build --release` in `host/`.
2. Copy `target/release/routedroidd` and `target/release/routedroid` to `~/.local/bin/`.
3. Copy `systemd/routedroid.service` to `~/.config/systemd/user/` and run `systemctl --user enable --now routedroid`.
4. The privileged helper (`routedroid-phase0-helper.socket`) must still be installed system-wide, as before.
5. Check with `routedroid version` and `routedroid devices`.
