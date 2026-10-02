#!/usr/bin/env bash
# Install Routedroid on this host from a release build (`cargo build --release`
# in host/):
#   - routedroid, routedroidd, routedroid-tui      -> $PREFIX/bin
#   - routedroid-helper                            -> $PREFIX/libexec/routedroid
#   - the helper's socket-activated template unit  -> /etc/systemd/system
#   - the daemon's user unit                       -> /etc/systemd/user
#   - group `routedroid` (owns the helper socket) and a deny-all policy
#
#   sudo ./install.sh [--uninstall]      (PREFIX defaults to /usr/local)
#
# Members of group `routedroid` can ask the helper for a TUN, a /32 route,
# proxy ARP, forwarding and a DHCP lease on any interface and address
# /etc/routedroid/helper.toml allows, and nothing else. A fresh install allows
# nothing; edit the policy to enable. Uninstall removes binaries and units and
# leaves the policy, /var/lib/routedroid and the group.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
RELEASE=$HERE/target/release
PREFIX=${PREFIX:-/usr/local}
BINDIR=$PREFIX/bin
LIBEXECDIR=$PREFIX/libexec/routedroid
UNIT=routedroid-helper
SYSTEM_UNITS=/etc/systemd/system
USER_UNITS=/etc/systemd/user
CLIENTS=(routedroid routedroidd routedroid-tui)
[[ $EUID -eq 0 ]] || { echo "run with sudo"; exit 2; }

if [[ ${1:-} == --uninstall ]]; then
    systemctl disable --now "$UNIT.socket" 2>/dev/null || true
    # Waits for every instance, ExecStopPost cleanup included, to finish.
    systemctl stop "$UNIT@*.service" 2>/dev/null || true
    # The binary is about to go: replay anything an instance left behind first.
    if [[ -x $LIBEXECDIR/routedroid-helper ]] && ! "$LIBEXECDIR/routedroid-helper" cleanup; then
        echo "warning: cleanup left journals in /var/lib/routedroid/journal; see above" >&2
    fi
    rm -f "$SYSTEM_UNITS/$UNIT.socket" "$SYSTEM_UNITS/$UNIT.service" "$SYSTEM_UNITS/$UNIT@.service"
    rm -f "$USER_UNITS/routedroid.service"
    systemctl daemon-reload
    for bin in "${CLIENTS[@]}"; do rm -f "$BINDIR/$bin"; done
    rm -rf "$LIBEXECDIR" /run/routedroid
    echo "removed binaries and units (/etc/routedroid, /var/lib/routedroid and group routedroid left in place)"
    echo "a running daemon keeps going until: systemctl --user disable --now routedroid"
    exit 0
fi

for bin in "${CLIENTS[@]}" routedroid-helper; do
    [[ -x $RELEASE/$bin ]] || { echo "build first: (cd host && cargo build --release)"; exit 2; }
done
getent group routedroid >/dev/null || groupadd --system routedroid
USER_TO_ADD=${SUDO_USER:-}
if [[ -n $USER_TO_ADD ]] && ! id -nG "$USER_TO_ADD" | tr ' ' '\n' | grep -qx routedroid; then
    usermod -aG routedroid "$USER_TO_ADD"
    echo "added $USER_TO_ADD to group routedroid: it may now have the helper attach phones to"
    echo "  the interfaces and addresses /etc/routedroid/helper.toml allows (proxy ARP for them"
    echo "  on that LAN). Takes effect in new logins; 'sg routedroid -c ...' or newgrp meanwhile."
fi
install -d -m 0755 "$BINDIR" "$LIBEXECDIR" /etc/routedroid "$USER_UNITS"
if [[ ! -e /etc/routedroid/helper.toml ]]; then
    install -m 0644 /dev/stdin /etc/routedroid/helper.toml <<'POLICY'
# Which LAN interfaces may carry phones, and which addresses phones may take
# there: requested ones inside `phone_addresses`, and with `dhcp = true` an
# address leased from the LAN's DHCP server (inside `phone_addresses` too, if
# any are listed). Owned by root, not writable by group or others, or the
# helper refuses every session. `routedroid interfaces` shows what it allows.
# Example:
#
# [[interface]]
# name = "eno1"
# phone_addresses = ["192.168.1.200/29"]
# dhcp = true
POLICY
    echo "wrote /etc/routedroid/helper.toml (allows nothing yet; add your LAN interface)"
fi
for bin in "${CLIENTS[@]}"; do install -m 0755 "$RELEASE/$bin" "$BINDIR/$bin"; done
install -m 0755 "$RELEASE/routedroid-helper" "$LIBEXECDIR/routedroid-helper"

unit() { # unit SOURCE DEST: install with the paths filled in
    sed -e "s|@BINDIR@|$BINDIR|g" -e "s|@LIBEXECDIR@|$LIBEXECDIR|g" "$1" | install -m 0644 /dev/stdin "$2"
}
# An older non-template unit must go: with Accept=yes systemd looks for the template.
systemctl disable --now "$UNIT.socket" 2>/dev/null || true
rm -f "$SYSTEM_UNITS/$UNIT.service"
unit "$HERE/routedroid-helper/systemd/$UNIT.socket" "$SYSTEM_UNITS/$UNIT.socket"
unit "$HERE/routedroid-helper/systemd/$UNIT@.service" "$SYSTEM_UNITS/$UNIT@.service"
unit "$HERE/routedroidd/systemd/routedroid.service" "$USER_UNITS/routedroid.service"
systemctl daemon-reload
systemctl enable --now "$UNIT.socket"
echo "$UNIT.socket: $(systemctl is-active "$UNIT.socket")"
echo
echo "installed. Next, as yourself (not root):"
echo "  systemctl --user enable --now routedroid"
echo "  routedroid interfaces        # then allow one in /etc/routedroid/helper.toml"
