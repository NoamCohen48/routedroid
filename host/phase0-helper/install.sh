#!/usr/bin/env bash
# Install the Phase 0 helper spike as a socket-activated systemd unit.
#   sudo ./install.sh [--uninstall]
# Adds the invoking user (SUDO_USER) to group `routedroid`, which owns the socket.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
BIN=$HERE/../target/release/phase0-helper
DEST=/usr/local/libexec/routedroid
UNIT=routedroid-phase0-helper
[[ $EUID -eq 0 ]] || { echo "run with sudo"; exit 2; }
if [[ ${1:-} == --uninstall ]]; then
    systemctl disable --now $UNIT.socket 2>/dev/null || true
    systemctl stop $UNIT.service 2>/dev/null || true
    rm -f /etc/systemd/system/$UNIT.socket /etc/systemd/system/$UNIT.service
    systemctl daemon-reload
    rm -rf "$DEST" /run/routedroid
    echo "removed units and $DEST (journal dir /var/lib/routedroid left in place; group routedroid left in place)"
    exit 0
fi
[[ -x $BIN ]] || { echo "build first: cargo build --release -p phase0-helper"; exit 2; }
getent group routedroid >/dev/null || groupadd --system routedroid
USER_TO_ADD=${SUDO_USER:-}
if [[ -n $USER_TO_ADD ]] && ! id -nG "$USER_TO_ADD" | tr ' ' '\n' | grep -qx routedroid; then
    usermod -aG routedroid "$USER_TO_ADD"
    echo "added $USER_TO_ADD to group routedroid (takes effect in new logins; use 'sg routedroid -c ...' or newgrp meanwhile)"
fi
install -d -m 0755 "$DEST"
install -m 0755 "$BIN" "$DEST/phase0-helper"
install -m 0644 "$HERE/systemd/$UNIT.socket" "$HERE/systemd/$UNIT.service" /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now $UNIT.socket
systemctl status --no-pager $UNIT.socket | head -5
echo "installed. controller: $DEST/phase0-helper client --lan-if IF --phone-ip IP"
