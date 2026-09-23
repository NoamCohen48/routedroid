#!/usr/bin/env bash
# Multi-session: two sessions on one LAN interface through one helper (the
# `--socket` stand-in for systemd's Accept=yes instances), run in an
# unprivileged user+network namespace.
#
#   ./multi-session.sh
#
# Checks: both TUNs and both per-session nft tables coexist; the LAN
# interface's proxy_arp/forwarding are claimed once and restored only when
# the last session ends (and left alone by the first one to end); a phone
# cannot reach the other phone through the host; a duplicate TUN name or
# phone address is refused; a session's crash leaves the other one intact.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
# Both binaries come from `cargo build --release -p routedroid-helper --features testing`.
BIN=${BIN:-$HERE/../../host/target/release/routedroid-helper}
CLIENT=${CLIENT:-$HERE/../../host/target/release/routedroid-helper-client}
S=$(mktemp -d /tmp/rd-multi.XXXXXX); echo "artifacts: $S"
pass=0; fail=0
check() { local name=$1; shift; if "$@"; then echo "PASS  $name"; pass=$((pass+1)); else echo "FAIL  $name"; fail=$((fail+1)); fi; }
LAN_IF=lan0; HOST_IP=10.90.0.1; A_IP=10.90.0.7; B_IP=10.90.0.8

unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
NSPID=$!; sleep 0.5
NS="nsenter -t $NSPID -U -n --preserve-credentials"
$NS ip link add $LAN_IF type dummy; $NS ip addr add $HOST_IP/24 dev $LAN_IF; $NS ip link set $LAN_IF up
HELPER="$NS $BIN --journal-dir $S/journal --claims-dir $S/claims --crash-file $S/crash-at"
$HELPER serve --socket "$S/helper.sock" > "$S/helper.log" 2>&1 &
HPID=$!
for _ in $(seq 1 30); do [[ -S $S/helper.sock ]] && break; sleep 0.1; done
trap 'kill $HPID 2>/dev/null; kill $NSPID 2>/dev/null; echo "artifacts in $S"' EXIT

sysctl_is() { [[ $($NS cat /proc/sys/net/ipv4/conf/$LAN_IF/$1) == "$2" ]]; }
# The namespace inherits the host's defaults, so "restored" means back to these, not to 0.
BASE_FWD=$($NS cat /proc/sys/net/ipv4/conf/$LAN_IF/forwarding); BASE_ARP=$($NS cat /proc/sys/net/ipv4/conf/$LAN_IF/proxy_arp)
echo "baseline: forwarding=$BASE_FWD proxy_arp=$BASE_ARP"
client() { # client NAME TUN PHONE_IP args...
    local name=$1 tun=$2 ip=$3; shift 3
    $NS "$CLIENT" --socket "$S/helper.sock" --lan-if $LAN_IF --phone-ip "$ip" --tun "$tun" "$@" > "$S/client-$name.log" 2>&1
}
# Background client whose pid ($!) is the client process itself (nsenter execs it), so it can be signalled.
client_bg() { local name=$1 tun=$2 ip=$3; shift 3; ( exec $NS "$CLIENT" --socket "$S/helper.sock" --lan-if $LAN_IF --phone-ip "$ip" --tun "$tun" "$@" > "$S/client-$name.log" 2>&1 ) & }
started() { for _ in $(seq 1 50); do grep -q ^STARTED "$S/client-$1.log" && return 0; sleep 0.1; done; return 1; }
claim_holders() { python3 -c "import json,sys; print(len(json.load(open(sys.argv[1]))['holders']))" "$S/claims/net.ipv4.conf.$LAN_IF.proxy_arp.json" 2>/dev/null || echo 0; }

echo "== two sessions up"
client_bg a phone0 $A_IP --hold 60; APID=$!
check "A started" started a
client_bg b phone1 $B_IP --hold 60; BPID=$!
check "B started" started b
check "both TUNs exist"            bash -c "$NS ip link show phone0 >/dev/null && $NS ip link show phone1 >/dev/null"
check "one nft table per session"  bash -c "$NS nft list tables | grep -q routedroid_phone0 && $NS nft list tables | grep -q routedroid_phone1"
check "proxy_arp on, held by two"  bash -c "$(declare -f sysctl_is claim_holders); NS='$NS'; LAN_IF=$LAN_IF; S=$S; sysctl_is proxy_arp 1 && [ \$(claim_holders) -eq 2 ]"
check "route per phone"            bash -c "$NS ip -4 route show $A_IP/32 | grep -q phone0 && $NS ip -4 route show $B_IP/32 | grep -q phone1"

echo "== phone A cannot reach phone B through the host"
client probe phone2 10.90.0.9 --bench 20 --bench-target $B_IP
check "probe got no replies"       grep -q "replies=0" "$S/client-probe.log"
check "forward chain dropped them" bash -c "$NS nft list table inet routedroid_phone2 | grep -A4 'chain forward' | grep 'iifname \"phone2\" counter packets' | grep -qv 'packets 0 '"
check "probe session torn down"    bash -c "! $NS ip link show phone2 >/dev/null 2>&1"

echo "== refusals"
client dup-tun phone0 10.90.0.10; check "duplicate TUN refused" grep -q "already exists" "$S/client-dup-tun.log"
client dup-ip phone3 $A_IP;      check "duplicate phone address refused" grep -q "already served" "$S/client-dup-ip.log"

echo "== first session ends; shared sysctls stay for the survivor"
kill -INT $BPID; wait $BPID; check "B stopped cleanly" grep -q ^STOPPED "$S/client-b.log"
check "phone1 gone, phone0 stays"  bash -c "! $NS ip link show phone1 >/dev/null 2>&1 && $NS ip link show phone0 >/dev/null"
check "B's nft table gone"         bash -c "! $NS nft list tables | grep -q routedroid_phone1"
check "proxy_arp still on, one holder" bash -c "$(declare -f sysctl_is claim_holders); NS='$NS'; LAN_IF=$LAN_IF; S=$S; sysctl_is proxy_arp 1 && [ \$(claim_holders) -eq 1 ]"
check "A's table and route intact" bash -c "$NS nft list table inet routedroid_phone0 >/dev/null && $NS ip -4 route show $A_IP/32 | grep -q phone0"

echo "== last session ends; baseline restored"
kill -INT $APID; wait $APID; check "A stopped cleanly" grep -q ^STOPPED "$S/client-a.log"
check "proxy_arp restored"         sysctl_is proxy_arp "$BASE_ARP"
check "forwarding restored"        sysctl_is forwarding "$BASE_FWD"
check "no claims left"             bash -c "! ls $S/claims/*.json >/dev/null 2>&1"
check "no journals left"           bash -c "! ls $S/journal/*.journal >/dev/null 2>&1"
check "no tables left"             bash -c "! $NS nft list tables | grep -q routedroid_"

echo "== one session's crash leaves the other intact"
client_bg a2 phone0 $A_IP --hold 60; APID=$!; started a2
client_bg b2 phone1 $B_IP --hold 60; BPID=$!; started b2
# The helper serves B's session in its own task; B's client dying undoes B only.
kill -KILL $BPID; wait $BPID 2>/dev/null; sleep 1
check "B's TUN gone after its client died" bash -c "! $NS ip link show phone1 >/dev/null 2>&1"
check "A's TUN and table survive"  bash -c "$NS ip link show phone0 >/dev/null && $NS nft list tables | grep -q routedroid_phone0"
check "proxy_arp still on for A"   sysctl_is proxy_arp 1
kill -INT $APID; wait $APID; check "A stopped cleanly" grep -q ^STOPPED "$S/client-a2.log"
check "baseline restored after all" bash -c "$(declare -f sysctl_is); NS='$NS'; LAN_IF=$LAN_IF; sysctl_is proxy_arp $BASE_ARP && sysctl_is forwarding $BASE_FWD"
check "helper check passes"        $HELPER check
echo "RESULT: $pass passed, $fail failed / artifacts in $S"
[[ $fail -eq 0 ]]
