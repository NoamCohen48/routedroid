#!/usr/bin/env bash
# Phase 0 §3.4: protected ADB-stdin bootstrap + mutual HMAC, against a real device or
# emulator, host side in an unprivileged user namespace (no root).
#
#   ./phone-bootstrap.sh SERIAL [happy|norecord|wrongsecret|replay|expired|forcestop|hostile|all]
#
# Each case starts a fresh host (one connection per host process), delivers (or withholds)
# the 80-byte record via `adb shell content write` stdin, launches BootstrapActivity, and
# checks logcat + host log. VPN consent must already be granted for happy/replay (accept once).
set -euo pipefail
SERIAL=${1:?serial}; CASE=${2:-all}
HERE=$(cd "$(dirname "$0")" && pwd)
BIN=${BIN:-$HERE/../../host/target/release/phase0-tunnel}
S=$(mktemp -d /tmp/rd-bs.XXXXXX)
PKG=dev.routedroid.phase0
URI=content://dev.routedroid.phase0.bootstrap/record
pass=0; fail=0
log() { printf '\n== %s\n' "$*"; }
check() { local name=$1; shift; if "$@"; then echo "PASS  $name"; pass=$((pass+1)); else echo "FAIL  $name"; fail=$((fail+1)); fi; }
ADB() { adb -s "$SERIAL" "$@"; }
export -f ADB

unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
NSPID=$!; sleep 0.5
NS="nsenter -t $NSPID -U -n --preserve-credentials"
TPID=""; SOCAT1=""; SOCAT2=""
stop_host() {
    set +e
    [[ -n $TPID ]] && kill -INT "$TPID" 2>/dev/null && for _ in $(seq 1 30); do kill -0 "$TPID" 2>/dev/null || break; sleep 0.1; done
    [[ -n $SOCAT1 ]] && kill "$SOCAT1" 2>/dev/null; [[ -n $SOCAT2 ]] && kill "$SOCAT2" 2>/dev/null
    TPID=""; SOCAT1=""; SOCAT2=""
    ADB reverse --remove tcp:9000 2>/dev/null
    set -e
}
cleanup() { trap - EXIT INT TERM; stop_host; kill "$NSPID" 2>/dev/null || true; echo; echo "artifacts in $S"; }
trap cleanup EXIT INT TERM

# start_host NAME SECRETFILE -> sets SESSION, LOG; host waits for the client
start_host() {
    local name=$1 secret=$2
    SESSION=p0-bs-$name-$(head -c3 /dev/urandom | xxd -p)
    LOG=$S/host-$name.log
    $NS "$BIN" run --no-adb --session "$SESSION" --secret-file "$secret" --tun phone0 \
        --address 10.77.0.2/32 --route 10.77.0.0/24 --dns 10.77.0.1 > "$LOG" 2>&1 &
    TPID=$!
    for _ in $(seq 1 50); do grep -q "waiting for a client" "$LOG" 2>/dev/null && break; sleep 0.1; done
    HP=$(sed 's/\x1b\[[0-9;]*m//g' "$LOG" | grep -oE 'host_port=[0-9]+' | grep -oE '[0-9]+$' | head -1)
    [[ -n $HP ]] || { cat "$LOG"; exit 2; }
    $NS socat UNIX-LISTEN:"$S/tun-$name.sock",fork,unlink-early TCP4:127.0.0.1:"$HP" & SOCAT1=$!
    sleep 0.3; socat TCP4-LISTEN:"$HP",bind=127.0.0.1,fork,reuseaddr UNIX:"$S/tun-$name.sock" & SOCAT2=$!
    sleep 0.3
    ADB reverse --remove-all >/dev/null 2>&1 || true
    ADB reverse tcp:9000 tcp:"$HP" >/dev/null
}
deliver() { "$BIN" bootstrap-record --session "$1" --secret-file "$2" | ADB shell content write --uri "$URI"; }
launch()  { ADB shell am start -n $PKG/.BootstrapActivity --es session "$1" --ei device_port 9000 >/dev/null; }
wait_log() { for _ in $(seq 1 "${3:-100}"); do ADB logcat -d -s "$1" 2>/dev/null | grep -q "$2" && return 0; sleep 0.1; done; return 1; }
lc() { ADB logcat -d -s Phase0Bootstrap:* Phase0BootstrapStore:* Phase0BootstrapProv:* Phase0Handshake:* Phase0Vpn:* HostileProbe:* 2>/dev/null; }
vpn_prompt_shown() { ADB shell dumpsys activity activities 2>/dev/null | grep -qiE "VpnDialogs|ConfirmDialog"; }
service_running() { ADB shell dumpsys activity services $PKG 2>/dev/null | grep -q "Phase0VpnService"; }
stop_session() { ADB shell am startservice -n $PKG/.Phase0VpnService -a dev.routedroid.phase0.STOP >/dev/null 2>&1 || true; sleep 1; }
export SERIAL PKG; export -f lc vpn_prompt_shown service_running

newsecret() { head -c32 /dev/urandom | xxd -p -c64 > "$1"; chmod 600 "$1"; }
SEC_A=$S/secret-a; SEC_B=$S/secret-b; newsecret "$SEC_A"; newsecret "$SEC_B"

run_case() {
    ADB logcat -c
    trap 'lc > "$S/logcat-$1.txt"' RETURN
    case $1 in
    happy)
        log "happy path: record via content write stdin, am start, mutual HMAC, session Active"
        start_host happy "$SEC_A"; deliver "$SESSION" "$SEC_A"; launch "$SESSION"
        check "provider stored record (shell uid)"  wait_log Phase0BootstrapStore "record stored" 50
        check "host authenticated on Android"       wait_log Phase0Handshake "host authenticated" 100
        for _ in $(seq 1 300); do grep -q "session Active" "$LOG" && break; sleep 0.1; done
        check "host: session Active"                grep -q "session Active" "$LOG"
        $NS ip addr add 10.77.0.1/24 dev phone0
        check "host: ICMP reaches phone"            bash -c "$NS ping -c 5 -W 1 10.77.0.2 | grep -qE ' [0-9]+ received' && ! $NS ping -c 2 -W 1 10.77.0.2 | grep -q '100% packet loss'"
        check "secret absent from host log"         bash -c "! grep -q \"\$(cat $SEC_A)\" $LOG"
        check "secret absent from logcat"           bash -c "! ADB logcat -d | grep -q \"\$(cat $SEC_A)\""
        check "secret absent from device cmdlines"  bash -c "! ADB shell 'ps -A -o ARGS' | grep -q \"\$(cat $SEC_A)\""
        ;;
    replay)
        log "replay: same session launched again while record already consumed"
        launch "$SESSION"; sleep 2
        check "second launch refused (no record)"   bash -c "lc | grep -c 'refusing launch' | grep -qE '^[1-9]'"
        check "host still Active (untouched)"       bash -c "! grep -q 'session ended' $LOG"
        stop_session; stop_host
        ;;
    norecord)
        log "no record: am start alone must not connect, prompt, or start the service"
        start_host norecord "$SEC_A"; launch "$SESSION"; sleep 3
        check "activity refused"                    wait_log Phase0Bootstrap "refusing launch" 20
        check "host never saw a client"             bash -c "! grep -q 'client connected' $LOG"
        check "no VPN prompt"                       bash -c "! vpn_prompt_shown"
        check "no service"                          bash -c "! service_running"
        stop_host
        ;;
    wrongsecret)
        log "wrong secret: record built from B, host holds A"
        start_host wrong "$SEC_A"; deliver "$SESSION" "$SEC_B"; launch "$SESSION"
        check "Android rejects host proof"          wait_log Phase0Bootstrap "host_proof does not verify" 100
        sleep 1
        check "host: peer closed before AUTH"       bash -c "grep -qE 'peer closed|session ended' $LOG && ! grep -q 'session Active' $LOG"
        check "no VPN prompt"                       bash -c "! vpn_prompt_shown"
        check "no service"                          bash -c "! service_running"
        stop_host
        ;;
    badproof)
        log "bad android proof: Android holds B, host holds A; host must reject AUTH"
        # (Same wire shape as wrongsecret from the host's view is impossible since Android
        #  verifies first; exercised by the Rust unit tests instead.)
        ;;
    expired)
        log "expired: record older than TTL (60 s) is refused"
        start_host expired "$SEC_A"; deliver "$SESSION" "$SEC_A"; echo "waiting 62 s ..."; sleep 62; launch "$SESSION"
        check "expired record refused"              wait_log Phase0BootstrapStore "record expired" 30
        check "host never saw a client"             bash -c "! grep -q 'client connected' $LOG"
        stop_host
        ;;
    forcestop)
        log "process death: record delivered, app force-stopped, launch must be refused"
        start_host fstop "$SEC_A"; deliver "$SESSION" "$SEC_A"; ADB shell am force-stop $PKG; sleep 1; launch "$SESSION"
        check "no record after process death"       wait_log Phase0BootstrapStore "no pending record" 30
        check "host never saw a client"             bash -c "! grep -q 'client connected' $LOG"
        stop_host
        ;;
    hostile)
        log "hostile app: provider write/read denied, activity launch without record refused"
        start_host hostile "$SEC_A"
        ADB shell am force-stop dev.routedroid.hostile   # fresh onCreate, not a resume
        ADB shell am start -n dev.routedroid.hostile/.HostileActivity --es session "$SESSION" --ei device_port 9000 >/dev/null
        sleep 3; lc | grep HOSTILE-RESULT || true
        check "forged record write denied"          bash -c "lc | grep -q 'HOSTILE-RESULT 1 PASS'"
        check "provider read denied"                bash -c "lc | grep -q 'HOSTILE-RESULT 2 PASS'"
        check "BootstrapActivity refused (no record)" wait_log Phase0Bootstrap "refusing launch" 20
        check "provider saw no shell record"        bash -c "! lc | grep -q 'record stored'"
        check "host never saw a client"             bash -c "! grep -q 'client connected' $LOG"
        check "no VPN prompt"                       bash -c "! vpn_prompt_shown"
        check "no service"                          bash -c "! service_running"
        stop_host
        ;;
    esac
}

if [[ $CASE == all ]]; then
    for c in norecord hostile wrongsecret forcestop happy replay expired; do run_case $c; done
else
    run_case "$CASE"
fi
echo; echo "RESULT: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
