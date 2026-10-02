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
# cannot reach the other phone through the host (its own routing table
# holds only the LAN); a duplicate TUN name or
# phone address is refused; one controller's death undoes only its session.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
# Both binaries come from `cargo build --release -p routedroid-helper --features testing`.
BIN=${BIN:-$HERE/../../host/target/release/routedroid-helper}
CLIENT=${CLIENT:-$HERE/../../host/target/release/routedroid-helper-client}
rig_tmp multi
LAN_IF=lan0; HOST_IP=10.90.0.1; A_IP=10.90.0.7; B_IP=10.90.0.8

userns_start || exit 1
in_ns ip link add $LAN_IF type dummy
in_ns ip addr add $HOST_IP/24 dev $LAN_IF
in_ns ip link set $LAN_IF up
printf '[[interface]]\nname = "%s"\nphone_addresses = ["10.90.0.0/24"]\n' $LAN_IF > "$S/helper.toml"
HELPER=("${NS[@]}" "$BIN" --state-dir "$S/state" --policy "$S/helper.toml" --crash-file "$S/crash-at")
"${HELPER[@]}" serve --socket "$S/helper.sock" > "$S/helper.log" 2>&1 &
HPID=$!
wait_for_socket "$S/helper.sock"
trap 'kill $HPID 2>/dev/null; kill $NSPID 2>/dev/null' EXIT

sysctl_of() { in_ns cat "/proc/sys/net/ipv4/conf/$LAN_IF/$1"; }
sysctl_is() { [[ $(sysctl_of "$1") == "$2" ]]; }
# The namespace inherits the host's defaults, so "restored" means back to these, not to 0.
BASE_FWD=$(sysctl_of forwarding); BASE_ARP=$(sysctl_of proxy_arp)
echo "baseline: forwarding=$BASE_FWD proxy_arp=$BASE_ARP"
client() { # client NAME TUN PHONE_IP args...
    local name=$1 tun=$2 ip=$3; shift 3
    in_ns "$CLIENT" --socket "$S/helper.sock" --lan-if $LAN_IF --phone-ip "$ip" --tun "$tun" "$@" > "$S/client-$name.log" 2>&1
}
# Background client whose pid ($!) is the client process itself (nsenter execs it), so it can be signalled.
client_bg() {
    local name=$1 tun=$2 ip=$3; shift 3
    ( exec "${NS[@]}" "$CLIENT" --socket "$S/helper.sock" --lan-if $LAN_IF --phone-ip "$ip" --tun "$tun" "$@" > "$S/client-$name.log" 2>&1 ) &
}
started() { local _; for _ in $(seq 1 50); do grep -q ^STARTED "$S/client-$1.log" && return 0; sleep 0.1; done; return 1; }
claim_holders() {
    python3 -c "import json,sys; print(len(json.load(open(sys.argv[1]))['holders']))" \
        "$S/state/sysctl/net.ipv4.conf.$LAN_IF.proxy_arp.json" 2>/dev/null || echo 0
}
link_up() { in_ns ip link show "$1" >/dev/null 2>&1; }
no_link() { ! link_up "$1"; }
has_table() { in_ns nft list tables | grep -q "routedroid_$1"; }
no_table() { ! has_table "$1"; }
routed_via() { in_ns ip -4 route show "$1/32" | grep -q "$2"; }
proxy_arp_held_by() { sysctl_is proxy_arp 1 && [[ $(claim_holders) -eq $1 ]]; }
none_left() { ! compgen -G "$1" >/dev/null; }
# The probe phone's own table: the LAN, and no route at all to B's TUN.
egress_via_lan() { in_ns ip -4 route get 10.90.0.50 from 10.90.0.9 iif phone2 | grep -q "dev $LAN_IF table"; }
no_path_to_b() { ! in_ns ip -4 route get "$B_IP" from 10.90.0.9 iif phone2 >/dev/null 2>&1; }
baseline() { sysctl_is proxy_arp "$BASE_ARP" && sysctl_is forwarding "$BASE_FWD"; }

echo "== two sessions up"
client_bg a phone0 $A_IP --hold 60; APID=$!
check "A started" started a
client_bg b phone1 $B_IP --hold 60; BPID=$!
check "B started" started b
check "both TUNs exist"            eval 'link_up phone0 && link_up phone1'
check "one nft table per session"  eval 'has_table phone0 && has_table phone1'
check "proxy_arp on, held by two"  proxy_arp_held_by 2
check "route per phone"            eval "routed_via $A_IP phone0 && routed_via $B_IP phone1"

echo "== phone A cannot reach phone B through the host"
client_bg probe phone2 10.90.0.9 --bench 20 --bench-target $B_IP --hold 2; PPID_=$!
check "probe started"              started probe
check "its egress is the LAN only" egress_via_lan
check "and B is no route for it"   no_path_to_b
wait $PPID_
check "probe got no replies"       grep -q "replies=0" "$S/client-probe.log"
check "probe session torn down"    eventually no_link phone2

echo "== refusals"
client dup-tun phone0 10.90.0.10; check "duplicate TUN refused" grep -q "already exists" "$S/client-dup-tun.log"
client dup-ip phone3 $A_IP;      check "duplicate phone address refused" grep -q "already has a host route" "$S/client-dup-ip.log"

echo "== first session ends; shared sysctls stay for the survivor"
kill -INT $BPID; wait $BPID; check "B stopped cleanly" grep -q ^STOPPED "$S/client-b.log"
check "phone1 gone, phone0 stays"  eval 'no_link phone1 && link_up phone0'
check "B's nft table gone"         no_table phone1
check "proxy_arp still on, one holder" proxy_arp_held_by 1
check "A's table and route intact" eval "has_table phone0 && routed_via $A_IP phone0"

echo "== last session ends; baseline restored"
kill -INT $APID; wait $APID; check "A stopped cleanly" grep -q ^STOPPED "$S/client-a.log"
check "proxy_arp restored"         sysctl_is proxy_arp "$BASE_ARP"
check "forwarding restored"        sysctl_is forwarding "$BASE_FWD"
check "no claims left"             none_left "$S/state/sysctl/*.json"
check "no journals left"           none_left "$S/state/journal/*.journal"
check "no tables left"             no_table ''

echo "== one controller's death undoes only its session"
client_bg a2 phone0 $A_IP --hold 60; APID=$!
check "A2 started" started a2
client_bg b2 phone1 $B_IP --hold 60; BPID=$!
check "B2 started" started b2
# The helper serves B's session in its own task; B's controller dying undoes B only.
kill -KILL $BPID; wait $BPID 2>/dev/null; sleep 1
check "B's TUN gone after its client died" no_link phone1
check "A's TUN and table survive"  eval 'link_up phone0 && has_table phone0'
check "proxy_arp still on for A"   sysctl_is proxy_arp 1
kill -INT $APID; wait $APID; check "A stopped cleanly" grep -q ^STOPPED "$S/client-a2.log"
check "baseline restored after all" baseline
check "helper check passes"        "${HELPER[@]}" check
rig_end
