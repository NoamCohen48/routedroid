#!/usr/bin/env bash
# The .deb's whole life on the `ubuntu` guest, the way a user lives it: a
# fresh install, the steps its message gives, a full phone session, an
# upgrade under a live session (it must survive: needrestart is told not to
# restart helper instances), removal under a live session (nothing left
# behind), and purge (no policy, state or group).
#
#   host/packaging/build.sh && PHONE=04e8:6860 ./lab.sh up router ubuntu
#   ./package.sh SERIAL
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: package.sh SERIAL}
DEB=$(ls "$HERE"/../../host/target/packages/routedroid_*_amd64.deb) || { echo "build it: host/packaging/build.sh"; exit 2; }
rig_tmp package
pc() { timeout 300 "$HERE/lab.sh" ssh ubuntu "$@"; }
slowly() { local _; for _ in $(seq 1 30); do "$@" && return 0; sleep 2; done; return 1; }
active() { pc routedroid status | grep -q " active "; }
apt() { pc "sudo DEBIAN_FRONTEND=noninteractive apt-get $* 2>&1"; }
session() { pc routedroid start -s "$SERIAL" --lan-if lan0 --detach > /dev/null && slowly active; }
nothing_left() {
    pc "! ip -4 rule | grep -q 'proto 82' && ! ip link show phone0 2>/dev/null && ! sudo nft list tables | grep -q routedroid"
}

echo "== a clean guest"
pc 'systemctl --user disable --now routedroid 2>/dev/null; sudo dpkg --purge routedroid >/dev/null 2>&1
    [ -x ~/routedroid/host/install.sh ] && sudo ~/routedroid/host/install.sh --uninstall --purge >/dev/null; true'
pc 'cat > /tmp/routedroid.deb' < "$DEB"
echo "== install, then its message's steps"
apt install -y /tmp/routedroid.deb > "$S/install"
check "the message says to join the group" grep -q 'usermod -aG routedroid' "$S/install"
check "the helper socket is listening"     pc systemctl is-active -q routedroid-helper.socket
# shellcheck disable=SC2016 # $USER and $(id -u) are the guest's
pc 'sudo usermod -aG routedroid "$USER" && sudo systemctl restart "user@$(id -u)"'
check "the daemon runs from /usr/bin"      pc 'systemctl --user enable --now routedroid 2>/dev/null && sleep 1 && systemctl --user show -p ExecStart routedroid | grep -q /usr/bin/routedroidd'
check "phone-session.sh passes"            env GUEST=ubuntu "$HERE/phone-session.sh" "$SERIAL"
echo "== upgrade under a live session"
check "a session is active"                session
apt install -y --reinstall /tmp/routedroid.deb > "$S/upgrade"
check "needrestart defers the helper"     eval "grep -A3 'restarts being deferred' '$S/upgrade' | grep -q routedroid-helper@"
check "the session is still active"        active
check "and still carries traffic"          pc "adb -s $SERIAL shell ping -c2 -W3 192.168.80.1 | grep -q ' 0% packet loss'"
echo "== remove under a live session"
apt remove -y routedroid > "$S/remove"
check "removal undid the session"          nothing_left
check "the policy and group are kept"      pc 'test -f /etc/routedroid/helper.toml && getent group routedroid'
echo "== purge"
apt purge -y routedroid > "$S/purge"
check "no policy, state or group"          pc '! test -e /etc/routedroid && ! test -e /var/lib/routedroid && ! getent group routedroid'
rig_end
