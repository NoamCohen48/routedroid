#!/usr/bin/env bash
# The app that routedroidd carries, on a real phone: with the app removed,
# a start installs it first (state `installing app`), the phone asks for VPN
# permission once (answered here through the UI), and the connection goes
# active; the next start finds it current and installs nothing.
#
#   ROUTEDROID_APK=$PWD/android/app/build/outputs/apk/release/app-release.apk host/packaging/build.sh
#   PHONE=04e8:6860 ./lab.sh up router ubuntu && ./app-install.sh SERIAL
#
# It leaves the phone with the carried (release) app: put a debug build back
# with `adb install -r` after `adb uninstall dev.routedroid`.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: app-install.sh SERIAL}
DEB=$(ls "$HERE"/../../host/target/packages/routedroid_*_amd64.deb) || { echo "build it: host/packaging/build.sh"; exit 2; }
rig_tmp app-install
pc() { timeout 300 "$HERE/lab.sh" ssh ubuntu "$@"; }
phone() { pc "adb -s $SERIAL shell $(printf '%q ' "$*")"; }
slowly() { local _; for _ in $(seq 1 30); do "$@" && return 0; sleep 2; done; return 1; }
state() { pc routedroid status | grep -q " $1 "; }
# The VPN consent dialog: wake and unlock (no PIN), then press its positive button.
consent() {
    phone input keyevent KEYCODE_WAKEUP; phone input swipe 360 1200 360 300 300; sleep 1
    local bounds
    bounds=$(pc "adb -s $SERIAL exec-out uiautomator dump /dev/tty" \
        | grep -o 'resource-id="android:id/button1"[^>]*bounds="[^"]*"' | grep -o '\[[0-9,]*\]\[[0-9,]*\]') || return 1
    read -r x1 y1 x2 y2 <<< "$(tr '[],' '   ' <<< "$bounds")"
    phone input tap $(((x1 + x2) / 2)) $(((y1 + y2) / 2))
}

echo "== the .deb that carries the app, on a clean guest"
pc 'systemctl --user disable --now routedroid 2>/dev/null; sudo dpkg --purge routedroid >/dev/null 2>&1
    [ -x ~/routedroid/host/install.sh ] && sudo ~/routedroid/host/install.sh --uninstall --purge >/dev/null; true'
pc 'cat > /tmp/routedroid.deb' < "$DEB"
pc 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall /tmp/routedroid.deb' > "$S/install" 2>&1
# shellcheck disable=SC2016 # $USER and $(id -u) are the guest's
pc 'sudo usermod -aG routedroid "$USER" && sudo systemctl restart "user@$(id -u)" && systemctl --user enable --now routedroid'
pc 'printf "[[interface]]\nname = \"lan0\"\ndhcp = true\n" | sudo tee /etc/routedroid/helper.toml >/dev/null'
check "the daemon carries an app"         eval "pc 'journalctl --user -u routedroid -n 20 --no-pager' | grep 'routedroidd ready' | tail -1 | grep -qv 'app=none'"

echo "== a phone without the app"
pc "adb -s $SERIAL uninstall dev.routedroid" > /dev/null 2>&1
check "the app is gone"                   eval "! phone pm path dev.routedroid | grep -q package:"
pc routedroid start -s "$SERIAL" --lan-if lan0 --detach > /dev/null
check "the connection installs the app"   slowly state installing
check "the app is on the phone"           slowly eval "phone pm path dev.routedroid | grep -q package:"
check "it asks for VPN permission"        slowly state handshaking
sleep 2
check "which is given"                    consent
check "the connection is active"          slowly state active
pc routedroid stop -s "$SERIAL" > /dev/null

echo "== the app is current"
SINCE=$(pc date +%s)
pc routedroid start -s "$SERIAL" --lan-if lan0 --detach > /dev/null
check "active again"                      slowly state active
check "with nothing installed"            eval "! pc 'journalctl --user -u routedroid --since @$SINCE --no-pager' | grep -q 'installing the app'"
check "stop"                              pc routedroid stop -s "$SERIAL"
rig_end
