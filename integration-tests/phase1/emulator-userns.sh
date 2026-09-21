#!/usr/bin/env bash
# Phase 1 end-to-end on an emulator without root: the helper runs in an
# unprivileged user+network namespace (owns a dummy `lan0` and the TUN), while
# `routedroid start` runs in the host namespace with the real adb server. The
# helper socket is a Unix path, so it crosses the namespace boundary.
#
#   emulator-userns.sh [SERIAL] [sigint|app]     # how the session is ended
#
# Needs: host/target/release/{routedroid,phase0-helper}, the app installed on
# the emulator (android/app/build/outputs/apk/debug/app-debug.apk), socat.
set -u
SERIAL=${1:-emulator-5554}; STOP_MODE=${2:-sigint}
H=$(cd "$(dirname "$0")/../../host" && pwd)/target/release
S=$(mktemp -d /tmp/rd-e2e.XXXXXX); echo "artifacts: $S"
PHONE_IP=10.90.0.7; HOST_IP=10.90.0.1
pass=0; fail=0
check() { local name=$1; shift; if "$@"; then echo "PASS  $name"; pass=$((pass+1)); else echo "FAIL  $name"; fail=$((fail+1)); fi; }

unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
NSPID=$!; sleep 0.5
NS="nsenter -t $NSPID -U -n --preserve-credentials"
$NS ip link add lan0 type dummy; $NS ip addr add $HOST_IP/24 dev lan0; $NS ip link set lan0 up
$NS "$H/phase0-helper" --journal-dir "$S/journal" --crash-file "$S/crash" serve --socket "$S/helper.sock" > "$S/helper.log" 2>&1 &
HPID=$!
for _ in $(seq 1 30); do [[ -S $S/helper.sock ]] && break; sleep 0.1; done
cleanup() { kill "$HPID" 2>/dev/null; kill "$NSPID" 2>/dev/null; }
trap cleanup EXIT

"$H/routedroid" --log debug start --serial "$SERIAL" --lan-if lan0 --phone-ip $PHONE_IP \
    --helper-socket "$S/helper.sock" --dns $HOST_IP > "$S/host.log" 2>&1 &
RPID=$!

# Tap the notification / VPN consent dialogs if they appear.
tap_button() { # tap_button TEXT -> 0 if tapped
    adb -s "$SERIAL" shell rm -f /sdcard/ui.xml
    adb -s "$SERIAL" shell uiautomator dump /sdcard/ui.xml >/dev/null 2>&1 || return 1
    local b
    b=$(adb -s "$SERIAL" shell cat /sdcard/ui.xml 2>/dev/null | grep -o "text=\"$1\"[^>]*bounds=\"\[[0-9]*,[0-9]*\]" | head -1 | grep -o '\[[0-9]*,[0-9]*\]' | tr -d '[]')
    [[ -n $b ]] || return 1
    adb -s "$SERIAL" shell input tap ${b/,/ }
}
for _ in $(seq 1 40); do
    sleep 1
    grep -q 'session Active' "$S/host.log" && break
    kill -0 $RPID 2>/dev/null || break
    for txt in Allow OK; do tap_button "$txt" && { echo "tapped $txt"; sleep 1; }; done
done
check "session Active" grep -q 'session Active' "$S/host.log"

if grep -q 'session Active' "$S/host.log"; then
    check "ping PC -> phone" $NS ping -c 3 -W 2 $PHONE_IP
    check "ping phone -> PC" adb -s "$SERIAL" shell ping -c 3 -W 2 $HOST_IP
    head -c 200000 /dev/urandom > "$S/blob"
    adb -s "$SERIAL" shell 'toybox nc -l -p 7000 > /data/local/tmp/blob' & sleep 1
    $NS sh -c "cat '$S/blob' | timeout 10 socat - TCP4:$PHONE_IP:7000"; sleep 1
    check "TCP PC -> phone 200 KB" [ "$(md5sum < "$S/blob" | cut -d' ' -f1)" = "$(adb -s "$SERIAL" shell md5sum /data/local/tmp/blob | cut -d' ' -f1)" ]
    if [[ $STOP_MODE == app ]]; then
        adb -s "$SERIAL" shell am start -n dev.routedroid/.ui.MainActivity >/dev/null; sleep 2
        tap_button Stop || tap_button STOP || echo "could not find the Stop button"
        check "app STOP reaches host" bash -c "for _ in \$(seq 1 20); do grep -q 'peer sent STOP' '$S/host.log' && exit 0; sleep 0.5; done; exit 1"
    else
        kill -INT $RPID
    fi
fi
wait $RPID; rc=$?
check "host exit 0" [ $rc -eq 0 ]
check "helper acknowledged Stop" grep -q 'helper session stopped' "$S/host.log"
check "reverse mapping removed" [ -z "$(adb -s "$SERIAL" reverse --list)" ]
check "TUN gone" bash -c "! $NS ip link show phone0 >/dev/null 2>&1"
sleep 2
check "app session ended cleanly" bash -c "adb -s '$SERIAL' logcat -d -s VpnService | tail -1 | grep -q 'ended cleanly'"
check "VPN address gone on phone" bash -c "! adb -s '$SERIAL' shell ip -4 addr | grep -q $PHONE_IP"
echo "RESULT: $pass passed, $fail failed / artifacts in $S"
[[ $fail -eq 0 ]]
