#!/usr/bin/env bash
# Install Routedroid on this host from a release build (`cargo build --release`
# in host/, or the binaries in bin/ of a release's tarball):
#   - routedroid, routedroidd, routedroid-tui      -> $PREFIX/bin
#   - routedroid-helper                            -> $PREFIX/libexec/routedroid
#   - the helper's socket-activated template unit  -> /etc/systemd/system
#   - the daemon's user unit                       -> /etc/systemd/user
#   - group `routedroid` (owns the helper socket) and a deny-all policy
#
#   sudo ./install.sh [--uninstall [--purge]]   (PREFIX defaults to /usr/local)
#
# Members of group `routedroid` can ask the helper for a TUN, a /32 route,
# proxy ARP, forwarding and a DHCP lease on any interface and address
# /etc/routedroid/helper.toml allows, and nothing else. A fresh install allows
# nothing; edit the policy to enable. Uninstall replays every journal, removes
# whatever else Routedroid provably left on the host (`doctor --repair`), then
# binaries and units; it leaves the policy, /var/lib/routedroid and the group
# unless --purge is given.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
RELEASE=$HERE/target/release
[[ -d $HERE/bin ]] && RELEASE=$HERE/bin  # an unpacked release tarball
PREFIX=${PREFIX:-/usr/local}
BINDIR=$PREFIX/bin
LIBEXECDIR=$PREFIX/libexec/routedroid
UNIT=routedroid-helper
SYSTEM_UNITS=/etc/systemd/system
USER_UNITS=/etc/systemd/user
CLIENTS=(routedroid routedroidd routedroid-tui)
[[ $EUID -eq 0 ]] || { echo "run with sudo"; exit 2; }
# Not booted with systemd (a container): install the units, start nothing.
systemd() { [[ ! -d /run/systemd/system ]] || systemctl "$@"; }
# user_managers ARGS: systemctl --user ARGS in every logged-in user's manager.
user_managers() {
    local unit user
    for unit in $(systemd list-units 'user@*.service' --state=running --no-legend --plain | cut -d' ' -f1); do
        user=$(id -nu "$(basename "$unit" .service | cut -d@ -f2)" 2>/dev/null) || continue
        systemd --user -M "$user@" "$@" 2>/dev/null || true
    done
}

if [[ ${1:-} == --uninstall ]]; then
    PURGE=0; [[ ${2:-} == --purge ]] && PURGE=1
    systemd disable --now "$UNIT.socket" 2>/dev/null || true
    # Waits for every instance, ExecStopPost cleanup included, to finish.
    systemd stop "$UNIT@*.service" 2>/dev/null || true
    # The binary is about to go: replay anything an instance left behind first.
    CLEAN=1
    if [[ -x $LIBEXECDIR/routedroid-helper ]]; then
        "$LIBEXECDIR/routedroid-helper" cleanup || CLEAN=0
        "$LIBEXECDIR/routedroid-helper" doctor --repair || CLEAN=0
    fi
    if [[ $CLEAN -eq 0 ]]; then
        echo "warning: something could not be undone; see above (state kept in /var/lib/routedroid)" >&2
        PURGE=0
    fi
    rm -f "$SYSTEM_UNITS/$UNIT.socket" "$SYSTEM_UNITS/$UNIT.service" "$SYSTEM_UNITS/$UNIT@.service"
    rm -f "$USER_UNITS/routedroid.service"
    systemd daemon-reload
    user_managers daemon-reload
    for bin in "${CLIENTS[@]}"; do rm -f "$BINDIR/$bin"; done
    rm -rf "$LIBEXECDIR" /run/routedroid
    if [[ $PURGE -eq 1 ]]; then
        rm -rf /etc/routedroid /var/lib/routedroid
        groupdel routedroid 2>/dev/null || true
        echo "removed binaries, units, the policy, the helper's state and group routedroid"
    else
        echo "removed binaries and units (/etc/routedroid, /var/lib/routedroid and group routedroid left in place)"
    fi
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
    echo "  on that LAN). Log out completely (or reboot) before starting the daemon: a running"
    echo "  systemd --user keeps its old groups, and so does every service it starts."
fi
install -d -m 0755 "$BINDIR" "$LIBEXECDIR" /etc/routedroid "$SYSTEM_UNITS" "$USER_UNITS"
if [[ ! -e /etc/routedroid/helper.toml ]]; then
    install -m 0644 "$HERE/routedroid-helper/helper.toml" /etc/routedroid/helper.toml
    echo "wrote /etc/routedroid/helper.toml (allows nothing yet; add your LAN interface)"
fi
for bin in "${CLIENTS[@]}"; do install -m 0755 "$RELEASE/$bin" "$BINDIR/$bin"; done
install -m 0755 "$RELEASE/routedroid-helper" "$LIBEXECDIR/routedroid-helper"

unit() { # unit SOURCE DEST: install with the paths filled in
    sed -e "s|@BINDIR@|$BINDIR|g" -e "s|@LIBEXECDIR@|$LIBEXECDIR|g" "$1" | install -m 0644 /dev/stdin "$2"
}
# An older non-template unit must go: with Accept=yes systemd looks for the template.
systemd disable --now "$UNIT.socket" 2>/dev/null || true
rm -f "$SYSTEM_UNITS/$UNIT.service"
unit "$HERE/routedroid-helper/systemd/$UNIT.socket" "$SYSTEM_UNITS/$UNIT.socket"
unit "$HERE/routedroid-helper/systemd/$UNIT@.service" "$SYSTEM_UNITS/$UNIT@.service"
unit "$HERE/routedroidd/systemd/routedroid.service" "$USER_UNITS/routedroid.service"
systemd daemon-reload
user_managers daemon-reload
systemd enable --now "$UNIT.socket"
echo "$UNIT.socket: $(systemd is-active "$UNIT.socket")"
echo
echo "installed. Next, as yourself (not root):"
echo "  systemctl --user enable --now routedroid"
echo "  routedroid interfaces        # then allow one in /etc/routedroid/helper.toml"
echo "  routedroid doctor            # adb, the helper, the policy, leftovers"
