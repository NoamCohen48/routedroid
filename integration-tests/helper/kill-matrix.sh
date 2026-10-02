#!/usr/bin/env bash
# Kill matrix: the journaled privileged helper survives SIGKILL at every
# write-ahead boundary and always returns the kernel to baseline.
#
#   ./kill-matrix.sh userns              # no root: lab netns, helper run directly,
#                                        #   cleanup invoked the way ExecStopPost would
#   sudo ./kill-matrix.sh systemd LAN_IF PHONE_IP   # real units on the real host netns
#                                        #   (/etc/routedroid/helper.toml must allow both)
#
# For every crash stage the helper (or client) is SIGKILLed there, cleanup runs,
# and route / nft / sysctl / link state is compared with the baseline snapshot.
set -euo pipefail
MODE=${1:?userns|systemd}
HERE=$(cd "$(dirname "$0")" && pwd)
# Both binaries come from `cargo build --release -p routedroid-helper --features testing`.
BIN=${BIN:-$HERE/../../host/target/release/routedroid-helper}
CLIENT=${CLIENT:-$HERE/../../host/target/release/routedroid-helper-client}
S=$(mktemp -d /tmp/rd-helper.XXXXXX)
pass=0; fail=0
log() { printf '\n== %s\n' "$*"; }
check() { local name=$1; shift; if "$@"; then echo "PASS  $name"; pass=$((pass+1)); else echo "FAIL  $name"; fail=$((fail+1)); fi; }

TUN=phone0
if [[ $MODE == userns ]]; then
    LAN_IF=lan0; PHONE_IP=10.90.0.7
    unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
    NSPID=$!; sleep 0.5
    NS="nsenter -t $NSPID -U -n --preserve-credentials"
    $NS ip link add $LAN_IF type dummy; $NS ip addr add 10.90.0.1/24 dev $LAN_IF; $NS ip link set $LAN_IF up
    SOCK=$S/helper.sock; CRASH=$S/crash-at
    printf '[[interface]]\nname = "%s"\nphone_addresses = ["%s/32"]\n' $LAN_IF $PHONE_IP > "$S/helper.toml"
    HELPER="$NS $BIN --state-dir $S/state --policy $S/helper.toml --crash-file $CRASH"
    SUDO=""
    # A killed helper leaves its socket file; wait for the new one, not that.
    start_helper() { rm -f "$SOCK"; $HELPER serve --once --socket "$SOCK" > "$S/helper-$1.log" 2>&1 & HPID=$!; for _ in $(seq 1 30); do [[ -S $SOCK ]] && break; sleep 0.1; done; }
    wait_helper_exit() { for _ in $(seq 1 100); do kill -0 "$HPID" 2>/dev/null || break; sleep 0.1; done; ! kill -0 "$HPID" 2>/dev/null; }
    run_cleanup() { $HELPER cleanup >> "$S/cleanup-$1.log" 2>&1; }
    run_check() { $HELPER check >/dev/null 2>&1; }
    kill_helper() { kill -KILL "$HPID" 2>/dev/null || true; }
    snapshot() { $NS ip -4 route show > "$1/route"; $NS nft list ruleset > "$1/nft" 2>/dev/null || true
                 for k in net.ipv4.conf.$LAN_IF.forwarding net.ipv4.conf.$LAN_IF.proxy_arp; do printf '%s=%s\n' "$k" "$($NS sysctl -n "$k")"; done > "$1/sysctl"
                 $NS ip -br link | awk '{print $1}' | sort > "$1/links"; }
    cleanup_all() { kill "$NSPID" 2>/dev/null || true; }
else
    [[ $EUID -eq 0 ]] || { echo "systemd mode needs root"; exit 2; }
    LAN_IF=${2:?lan if}; PHONE_IP=${3:?phone ip}
    UNIT=routedroid-helper
    SOCK=/run/routedroid/helper.sock; CRASH=/run/routedroid/crash-at
    CLIENT_USER=${SUDO_USER:-$USER}
    systemctl is-active --quiet $UNIT.socket || { echo "$UNIT.socket not active; run host/install.sh"; exit 2; }
    HELPER="$BIN --crash-file $CRASH"
    NS=""
    start_helper() { systemctl reset-failed $UNIT.service $UNIT.socket 2>/dev/null || true; :; }   # socket activation starts it on connect
    # "deactivating" still counts as running: ExecStopPost=cleanup is in flight.
    unit_settled() { case $(systemctl show -p ActiveState --value $UNIT.service) in inactive|failed) return 0;; *) return 1;; esac; }
    wait_helper_exit() { for _ in $(seq 1 100); do unit_settled && break; sleep 0.1; done; unit_settled; }
    run_cleanup() { :; }     # ExecStopPost already ran; journalctl shows it
    run_check() { $HELPER check >/dev/null 2>&1; }
    kill_helper() { systemctl kill -s KILL $UNIT.service 2>/dev/null || true; }
    # Live counters (Docker/firewalld chains) change on their own; compare structure only.
    snapshot() { ip -4 route show > "$1/route"; nft list ruleset 2>/dev/null | sed -E 's/counter packets [0-9]+ bytes [0-9]+/counter/g' > "$1/nft" || true
                 for k in net.ipv4.conf.$LAN_IF.forwarding net.ipv4.conf.$LAN_IF.proxy_arp; do printf '%s=%s\n' "$k" "$(sysctl -n "$k")"; done > "$1/sysctl"
                 ip -br link | awk '{print $1}' | sort > "$1/links"; }
    cleanup_all() { rm -f "$CRASH"; }
fi
trap 'rm -f "$CRASH" 2>/dev/null; cleanup_all; echo; echo "artifacts in $S"' EXIT

client() { # client NAME args...   (runs as the unprivileged user in systemd mode)
    local name=$1; shift
    if [[ $MODE == systemd ]]; then sudo -u "$CLIENT_USER" "$CLIENT" --socket "$SOCK" --lan-if "$LAN_IF" --phone-ip "$PHONE_IP" --tun $TUN "$@" > "$S/client-$name.log" 2>&1
    else "$CLIENT" --socket "$SOCK" --lan-if "$LAN_IF" --phone-ip "$PHONE_IP" --tun $TUN "$@" > "$S/client-$name.log" 2>&1; fi
}
baseline_ok() { snapshot "$S/after"; diff -r "$S/before" "$S/after" > "$S/diff-$1.txt" && [[ ! -e /sys/class/net/$TUN || $MODE == userns ]]; }
tun_absent() { if [[ $MODE == userns ]]; then ! $NS ip link show $TUN >/dev/null 2>&1; else ! ip link show $TUN >/dev/null 2>&1; fi; }

mkdir -p "$S/before" "$S/after"; snapshot "$S/before"
echo "baseline: $(tr '\n' ' ' < "$S/before/sysctl")"

# ---------------------------------------------------------------- happy path
log "happy path: start, 2000 echo requests through the relay, stop"
rm -f "$CRASH"; start_helper happy
client happy --bench 2000 || true
grep -E "STARTED|BENCH|STOPPED" "$S/client-happy.log" || true
check "session started"                 grep -q "^STARTED" "$S/client-happy.log"
check "2000/2000 echo replies via relay" grep -q "replies=2000" "$S/client-happy.log"
check "clean stop acknowledged"         grep -q "^STOPPED" "$S/client-happy.log"
check "helper exited"                   wait_helper_exit
check "journal resolved (check passes)" run_check
check "baseline restored"               baseline_ok happy
check "tun gone"                        tun_absent

# ------------------------------------------------- client dies, helper alive
for stage in after_start during_traffic before_stop; do
    log "client SIGKILL at $stage"
    rm -f "$CRASH"; start_helper "c-$stage"
    client "c-$stage" --bench 400 --hold 1 --crash-at "$stage" || true
    check "helper noticed disconnect and exited" wait_helper_exit
    check "check passes (journal resolved)"      run_check
    check "baseline restored"                    baseline_ok "c-$stage"
    check "tun gone"                             tun_absent
done
log "client disconnects without Stop"
rm -f "$CRASH"; start_helper c-nostop
client c-nostop --no-stop || true
check "helper exited"     wait_helper_exit
check "check passes"      run_check
check "baseline restored" baseline_ok c-nostop

# --------------------------------------------- helper dies at every boundary
OPS=("tun:$TUN" "nft:inet:routedroid_$TUN" "sysctl:net.ipv4.conf.$TUN.forwarding" "sysctl:net.ipv4.conf.$LAN_IF.forwarding" "sysctl:net.ipv4.conf.$LAN_IF.proxy_arp" "route:$PHONE_IP/32@$TUN")
STAGES=()
for op in "${OPS[@]}"; do STAGES+=("pending:$op" "applied:$op" "done:$op"); done
STAGES+=("active")
for op in "${OPS[@]}"; do STAGES+=("undo_pending:$op" "undo_applied:$op" "undone:$op"); done
for stage in "${STAGES[@]}"; do
    log "helper SIGKILL at $stage"
    echo "$stage" > "$CRASH"; [[ $MODE == systemd ]] && chmod 600 "$CRASH"
    tag="h-${stage//[^A-Za-z0-9_.-]/_}"
    start_helper "$tag"
    client "$tag" --bench 200 --hold 1 || true
    check "helper is gone"                        wait_helper_exit
    rm -f "$CRASH"
    if [[ $MODE == userns ]]; then
        # ExecStopPost stand-in: cleanup must succeed, then check must pass.
        check "cleanup exit 0"                    run_cleanup "$tag"
    fi
    check "check passes (journal resolved)"       run_check
    check "baseline restored"                     baseline_ok "$tag"
    check "tun gone"                              tun_absent
    if [[ $stage == active || $stage == undo* || $stage == done:route* ]]; then
        check "client saw the helper vanish"     grep -q 'helper closed' "$S/client-$tag.log"
    fi
done

# ------------------------------------ an unresolved journal until serve starts
log "check fails while a journal is unresolved; the next serve cleans it up"
echo "applied:route:$PHONE_IP/32@$TUN" > "$CRASH"; [[ $MODE == systemd ]] && chmod 600 "$CRASH"
start_helper blocked; client blocked --hold 1 || true; wait_helper_exit || true; rm -f "$CRASH"
if [[ $MODE == userns ]]; then
    check "check fails before cleanup"           bash -c "! $HELPER check >/dev/null 2>&1"
    start_helper recovered; kill -TERM "$HPID"
    check "serve cleaned up and stopped"         wait_helper_exit
fi
check "check passes after cleanup"               run_check
check "baseline restored"                        baseline_ok blocked

echo; echo "RESULT: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
