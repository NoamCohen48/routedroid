#!/usr/bin/env bash
# Phase 0 §3.1 namespace lab: phone-ns <-> host-ns <-> lan-ns.
#
#   phone-ns: phonetun0 (fake Android VPN tun, 192.168.10.74/32, default via tun)
#   host-ns : phone0 (Rust TUN) ; hlhost 192.168.10.1/24 ; mgmt0 10.99.0.1/24 (unselected)
#   lan-ns  : hllan 192.168.10.50/24
#
# The "adb reverse" hop is modelled by fake_android.py creating its TUN in
# phone-ns and then setns()-ing into host-ns to connect to 127.0.0.1:HOST_PORT.
#
# Needs root. Prints PASS/FAIL per check, exits non-zero on any FAIL.
set -u -o pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
BIN=${PHASE0_TUNNEL:-$REPO/host/target/debug/phase0-tunnel}
FAKE=$HERE/fake_android.py

NS_PHONE=rd0phone
NS_HOST=rd0host
NS_LAN=rd0lan
VETH_HOST=hlhost
VETH_LAN=hllan
MGMT=mgmt0
HOST_TUN=phone0
PHONE_TUN=phonetun0
HOST_IP=192.168.10.1
LAN_IP=192.168.10.50
LAN_NET=192.168.10.0/24
PHONE_IP=192.168.10.74
SPOOF_IP=192.168.10.99
MGMT_IP=10.99.0.1
MTU=1400
SESSION="lab-$$-$RANDOM"

TMP=$(mktemp -d /tmp/rd0-lab.XXXXXX)
FAILS=0
PASSES=0
TUNNEL_PID=""
FAKE_PID=""
SERVER_PIDS=()

log()  { printf '\033[1;34m[lab]\033[0m %s\n' "$*"; }
pass() { printf '\033[1;32mPASS\033[0m  %s\n' "$*"; PASSES=$((PASSES + 1)); }
fail() { printf '\033[1;31mFAIL\033[0m  %s\n' "$*"; FAILS=$((FAILS + 1)); }
check() { # check "name" cmd...
    local name=$1; shift
    if "$@" >"$TMP/check.out" 2>&1; then pass "$name"; else fail "$name"; sed 's/^/      | /' "$TMP/check.out" | tail -n 8; fi
}
in_phone() { ip netns exec "$NS_PHONE" "$@"; }
in_host()  { ip netns exec "$NS_HOST" "$@"; }
in_lan()   { ip netns exec "$NS_LAN" "$@"; }

# ------------------------------------------------------------- preconditions
if [[ $EUID -ne 0 ]]; then echo "must run as root (sudo $0)"; exit 2; fi
for t in ip nft python3 ping curl; do command -v "$t" >/dev/null || { echo "missing tool: $t"; exit 2; }; done
if [[ ! -x $BIN ]]; then
    echo "phase0-tunnel binary not found at $BIN; build it first: (cd $REPO/host && cargo build)"; exit 2
fi
for ns in $NS_PHONE $NS_HOST $NS_LAN; do
    if ip netns list | grep -qw "$ns"; then echo "namespace $ns already exists; refusing to run"; exit 2; fi
done
for dev in $HOST_TUN $VETH_HOST $VETH_LAN $MGMT; do
    if ip link show "$dev" >/dev/null 2>&1; then echo "root netns already has $dev; refusing"; exit 2; fi
done

# ------------------------------------------------------------------ baseline
snapshot() { # snapshot <dir>
    local d=$1; mkdir -p "$d"
    ip -4 route show table all >"$d/routes"
    ip -o link show | awk -F': ' '{print $2}' | sort >"$d/links"
    nft list ruleset >"$d/nft" 2>/dev/null || true
    for k in net.ipv4.ip_forward net.ipv4.conf.all.forwarding net.ipv4.conf.all.proxy_arp \
             net.ipv4.conf.all.rp_filter net.ipv4.conf.default.forwarding net.ipv4.conf.default.proxy_arp; do
        sysctl -n "$k" 2>/dev/null | sed "s/^/$k=/"
    done >"$d/sysctl"
    ip netns list | sort >"$d/netns"
    ip -o addr show | awk '{print $2, $4}' | sort >"$d/addrs"
}
log "recording root-namespace baseline"
snapshot "$TMP/before"

# ------------------------------------------------------------------- cleanup
teardown_done=0
teardown() {
    [[ $teardown_done -eq 1 ]] && return
    teardown_done=1
    log "teardown"
    for p in "${SERVER_PIDS[@]:-}"; do [[ -n $p ]] && kill "$p" 2>/dev/null; done
    if [[ -n $TUNNEL_PID ]] && kill -0 "$TUNNEL_PID" 2>/dev/null; then
        kill -INT "$TUNNEL_PID" 2>/dev/null; sleep 1; kill -KILL "$TUNNEL_PID" 2>/dev/null
    fi
    if [[ -n $FAKE_PID ]] && kill -0 "$FAKE_PID" 2>/dev/null; then
        kill -TERM "$FAKE_PID" 2>/dev/null; sleep 0.5; kill -KILL "$FAKE_PID" 2>/dev/null
    fi
    wait 2>/dev/null
    for ns in $NS_PHONE $NS_HOST $NS_LAN; do ip netns del "$ns" 2>/dev/null; done
}
on_exit() {
    local rc=$?
    teardown
    if [[ $rc -ne 0 && $FAILS -eq 0 ]]; then fail "script aborted (rc=$rc)"; fi
    rm -rf "$TMP"
    if [[ $FAILS -eq 0 ]]; then log "ALL $PASSES CHECKS PASSED"; exit 0; else log "$FAILS FAILED, $PASSES passed"; exit 1; fi
}
trap on_exit EXIT
trap 'exit 130' INT TERM

# ---------------------------------------------------------------- namespaces
log "building namespaces"
ip netns add $NS_PHONE
ip netns add $NS_HOST
ip netns add $NS_LAN
for ns in $NS_PHONE $NS_HOST $NS_LAN; do ip -n $ns link set lo up; done

ip link add $VETH_HOST netns $NS_HOST type veth peer name $VETH_LAN netns $NS_LAN
ip -n $NS_HOST addr add $HOST_IP/24 dev $VETH_HOST
ip -n $NS_HOST link set $VETH_HOST up
ip -n $NS_LAN addr add $LAN_IP/24 dev $VETH_LAN
ip -n $NS_LAN link set $VETH_LAN up

# An unselected host-local address (management/container/VPN stand-in).
ip -n $NS_HOST link add $MGMT type dummy
ip -n $NS_HOST addr add $MGMT_IP/24 dev $MGMT
ip -n $NS_HOST link set $MGMT up

# Start from a known state: no global forwarding in host-ns, so the test
# proves whether per-interface forwarding is sufficient (§12 question).
in_host sysctl -q -w net.ipv4.ip_forward=0
log "host-ns net.ipv4.ip_forward=$(in_host sysctl -n net.ipv4.ip_forward) all.rp_filter=$(in_host sysctl -n net.ipv4.conf.all.rp_filter) default.rp_filter=$(in_host sysctl -n net.ipv4.conf.default.rp_filter)"

# -------------------------------------------------- deny-first nftables (§11)
log "installing deny-first nftables ruleset in host-ns"
cat >"$TMP/rules.nft" <<EOF
table inet routedroid {
    counter spoof_drop {}
    counter input_drop {}
    counter forward_drop {}
    counter postrouting_drop {}
    counter output_drop {}
    counter phone_to_lan {}
    counter lan_to_phone {}
    counter phone_to_host {}
    counter host_to_phone {}

    # raw prerouting: anti-spoof before conntrack and route classification.
    chain raw_prerouting {
        type filter hook prerouting priority -300; policy accept;
        iifname "$HOST_TUN" ip saddr != $PHONE_IP counter name "spoof_drop" drop
    }
    # input: alias may reach only the selected interface's host address.
    chain input {
        type filter hook input priority -10; policy accept;
        iifname "$HOST_TUN" ip saddr $PHONE_IP ip daddr $HOST_IP counter name "phone_to_host" accept
        iifname "$HOST_TUN" counter name "input_drop" drop
    }
    # forward: exact tun/physical/alias pairs only, terminal drops.
    chain forward {
        type filter hook forward priority -10; policy accept;
        iifname "$HOST_TUN" oifname "$VETH_HOST" ip saddr $PHONE_IP ip daddr $LAN_NET counter name "phone_to_lan" accept
        iifname "$VETH_HOST" oifname "$HOST_TUN" ip saddr $LAN_NET ip daddr $PHONE_IP counter name "lan_to_phone" accept
        iifname "$HOST_TUN" counter name "forward_drop" drop
        oifname "$HOST_TUN" counter name "forward_drop" drop
    }
    chain postrouting {
        type filter hook postrouting priority -10; policy accept;
        oifname "$HOST_TUN" ip daddr != $PHONE_IP counter name "postrouting_drop" drop
    }
    chain output {
        type filter hook output priority -10; policy accept;
        oifname "$HOST_TUN" ip daddr $PHONE_IP counter name "host_to_phone" accept
        oifname "$HOST_TUN" counter name "output_drop" drop
    }
}
EOF
in_host nft -f "$TMP/rules.nft"
check "nftables table inet routedroid installed in host-ns" in_host nft list table inet routedroid

counter() { in_host nft -j list counter inet routedroid "$1" | python3 -c 'import json,sys; d=json.load(sys.stdin); print([o["counter"]["packets"] for o in d["nftables"] if "counter" in o][0])'; }

# ------------------------------------------------------------------- tunnel
start_tunnel() { # start_tunnel <logfile>; sets TUNNEL_PID and HOST_PORT
    # Background processes are started via `ip netns exec` directly (not the
    # in_* helpers) so that $! is the real PID and signals reach it.
    ip netns exec $NS_HOST "$BIN" run --no-adb --tun $HOST_TUN --address $PHONE_IP/32 --mtu $MTU --route 0.0.0.0/0 \
        --dns $HOST_IP --session "$SESSION" >"$1" 2>&1 &
    TUNNEL_PID=$!
    HOST_PORT=""
    for _ in $(seq 1 50); do grep -q '^HOST_PORT=' "$1" 2>/dev/null && break; sleep 0.1; done
    HOST_PORT=$(sed -n 's/^HOST_PORT=//p' "$1" | head -1)
}
start_fake() { # start_fake <logfile> <readyfile>; sets FAKE_PID
    python3 "$FAKE" --port "$HOST_PORT" --session "$SESSION" --device-port 9000 \
        --tun-name $PHONE_TUN --tun-netns $NS_PHONE --connect-netns $NS_HOST \
        --ready-file "$2" >"$1" 2>&1 &
    FAKE_PID=$!
    for _ in $(seq 1 50); do [[ -f $2 ]] && break; sleep 0.1; done
}
wait_pid() { # wait_pid <pid> <seconds>; sets WAIT_RC to the exit code, 255 if still running
    local p=$1 n=$(( $2 * 10 ))
    WAIT_RC=255
    for _ in $(seq 1 "$n"); do
        if ! kill -0 "$p" 2>/dev/null; then wait "$p" 2>/dev/null; WAIT_RC=$?; return; fi
        sleep 0.1
    done
}

log "starting phase0-tunnel in host-ns"
start_tunnel "$TMP/tunnel.log"
if [[ -z $HOST_PORT ]]; then fail "tunnel did not print HOST_PORT"; cat "$TMP/tunnel.log"; exit 1; fi
pass "tunnel listening on 127.0.0.1:$HOST_PORT in host-ns"
check "host TUN $HOST_TUN exists in host-ns, up, mtu $MTU" \
    bash -c "ip -n $NS_HOST -d link show $HOST_TUN | grep -q 'mtu $MTU' && ip -n $NS_HOST -d link show $HOST_TUN | grep -q 'tun type tun'"
check "host TUN has no packet-information header (no 'pi' flag)" \
    bash -c "ip -n $NS_HOST -d link show $HOST_TUN | grep 'tun type tun' | grep -q 'pi off'"

# ------------------------------------------------------------- fake android
log "starting fake Android (tun in $NS_PHONE, socket in $NS_HOST)"
start_fake "$TMP/fake.log" "$TMP/ready"
if [[ ! -f $TMP/ready ]]; then fail "fake Android never reached VPN_READY"; cat "$TMP/fake.log" "$TMP/tunnel.log"; exit 1; fi
pass "handshake HELLO -> HELLO_ACK -> CONFIGURE_VPN -> VPN_READY completed"
sleep 0.3
check "tunnel reports session Active" grep -q "session Active" "$TMP/tunnel.log"
check "phone-ns has $PHONE_TUN with $PHONE_IP/32 and default route via it" \
    bash -c "ip -n $NS_PHONE -4 addr show $PHONE_TUN | grep -q '$PHONE_IP/32' && ip -n $NS_PHONE route show default | grep -q $PHONE_TUN"

# ----------------------------------------------- host routing (§9, §10, §12)
log "installing /32 route, per-interface forwarding and proxy ARP in host-ns"
in_host sysctl -q -w net.ipv4.conf.$HOST_TUN.forwarding=1
in_host sysctl -q -w net.ipv4.conf.$VETH_HOST.forwarding=1
in_host sysctl -q -w net.ipv4.conf.$VETH_HOST.proxy_arp=1
ip -n $NS_HOST route add $PHONE_IP/32 dev $HOST_TUN src $HOST_IP
check "host route $PHONE_IP/32 dev $HOST_TUN src $HOST_IP" \
    bash -c "ip -n $NS_HOST route get $PHONE_IP | grep -q 'dev $HOST_TUN'"
check "host-ns global ip_forward still 0 (only per-interface forwarding enabled)" \
    bash -c "[[ \$(ip netns exec $NS_HOST sysctl -n net.ipv4.ip_forward) == 0 ]]"

# --------------------------------------------------------------------- tests
PING="ping -c 3 -i 0.2 -W 2 -q"
check "ICMP phone -> PC ($HOST_IP)"          in_phone $PING $HOST_IP
check "ICMP phone -> LAN host ($LAN_IP)"     in_phone $PING $LAN_IP
check "ICMP LAN host -> phone ($PHONE_IP)"   in_lan   $PING $PHONE_IP
check "ICMP PC -> phone ($PHONE_IP)"         in_host  $PING $PHONE_IP
check "LAN host resolved phone via proxy ARP on $VETH_HOST (neighbour entry has host MAC)" \
    bash -c "ip -n $NS_LAN neigh show $PHONE_IP | grep -q \"\$(ip -n $NS_HOST -o link show $VETH_HOST | sed -n 's/.*link\/ether \([0-9a-f:]*\).*/\1/p')\""

# TCP: phone serves HTTP, LAN host and PC fetch it.
mkdir -p "$TMP/www" && echo "hello-from-phone-$SESSION" >"$TMP/www/hello.txt"
ip netns exec $NS_PHONE python3 -m http.server 8080 --bind $PHONE_IP --directory "$TMP/www" >"$TMP/phone-http.log" 2>&1 &
SERVER_PIDS+=($!)
# TCP: LAN host serves HTTP; the phone fetches it, and the access log proves no NAT.
mkdir -p "$TMP/www-lan" && echo "hello-from-lan-$SESSION" >"$TMP/www-lan/hello.txt"
ip netns exec $NS_LAN python3 -m http.server 8081 --bind $LAN_IP --directory "$TMP/www-lan" >"$TMP/lan-http.log" 2>&1 &
SERVER_PIDS+=($!)
# UDP echo on the phone.
ip netns exec $NS_PHONE python3 -c "
import socket
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.bind(('$PHONE_IP',9999))
while True:
    d,a=s.recvfrom(65535); s.sendto(d,a)
" &
SERVER_PIDS+=($!)
sleep 0.7

check "TCP LAN host -> phone HTTP (curl)" \
    bash -c "[[ \$(ip netns exec $NS_LAN curl -s --max-time 5 http://$PHONE_IP:8080/hello.txt) == hello-from-phone-$SESSION ]]"
check "TCP PC -> phone HTTP (curl)" \
    bash -c "[[ \$(ip netns exec $NS_HOST curl -s --max-time 5 http://$PHONE_IP:8080/hello.txt) == hello-from-phone-$SESSION ]]"
check "TCP phone -> LAN host HTTP (curl)" \
    bash -c "[[ \$(ip netns exec $NS_PHONE curl -s --max-time 5 http://$LAN_IP:8081/hello.txt) == hello-from-lan-$SESSION ]]"
sleep 0.3
check "no NAT: LAN server saw the original phone address $PHONE_IP" \
    grep -q "^$PHONE_IP - - " "$TMP/lan-http.log"
check "UDP LAN host -> phone echo" \
    in_lan python3 -c "
import socket,sys
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.settimeout(3)
s.sendto(b'udp-probe-$SESSION',('$PHONE_IP',9999))
d,a=s.recvfrom(65535)
sys.exit(0 if d==b'udp-probe-$SESSION' and a[0]=='$PHONE_IP' else 1)
"
# Larger-than-MTU transfer: forces fragmentation/segmentation through the 1400 tun.
head -c 200000 /dev/urandom >"$TMP/www/big.bin"
check "TCP 200 KiB transfer LAN host -> phone matches" \
    bash -c "ip netns exec $NS_LAN curl -s --max-time 10 http://$PHONE_IP:8080/big.bin | cmp -s - $TMP/www/big.bin"

# ------------------------------------------------ isolation (§11 acceptance)
spoof_before=$(counter spoof_drop)
ip -n $NS_PHONE addr add $SPOOF_IP/32 dev $PHONE_TUN
check "spoofed source $SPOOF_IP -> LAN host is NOT delivered" \
    bash -c "! ip netns exec $NS_PHONE ping -c 2 -i 0.2 -W 1 -q -I $SPOOF_IP $LAN_IP"
check "spoofed source $SPOOF_IP -> PC is NOT delivered" \
    bash -c "! ip netns exec $NS_PHONE ping -c 2 -i 0.2 -W 1 -q -I $SPOOF_IP $HOST_IP"
spoof_after=$(counter spoof_drop)
check "raw prerouting spoof counter increased ($spoof_before -> $spoof_after)" \
    bash -c "[[ $spoof_after -ge $((spoof_before + 4)) ]]"
ip -n $NS_PHONE addr del $SPOOF_IP/32 dev $PHONE_TUN

input_before=$(counter input_drop)
check "valid alias -> unselected host-local $MGMT_IP is NOT delivered" \
    bash -c "! ip netns exec $NS_PHONE ping -c 2 -i 0.2 -W 1 -q $MGMT_IP"
input_after=$(counter input_drop)
check "input chain dropped alias -> unselected address ($input_before -> $input_after)" \
    bash -c "[[ $input_after -ge $((input_before + 2)) ]]"
check "no packets hit the forward terminal drops during valid traffic" \
    bash -c "[[ \$(ip netns exec $NS_HOST nft -j list counter inet routedroid forward_drop | python3 -c 'import json,sys; d=json.load(sys.stdin); print([o[\"counter\"][\"packets\"] for o in d[\"nftables\"] if \"counter\" in o][0]') -eq 0 ]]"

# ------------------------------------------------------------------ teardown
log "stopping tunnel with SIGINT (graceful STOP)"
for p in "${SERVER_PIDS[@]}"; do kill "$p" 2>/dev/null; done
SERVER_PIDS=()
kill -INT "$TUNNEL_PID"
wait_pid "$TUNNEL_PID" 5; tunnel_rc=$WAIT_RC
check "tunnel exited 0 on SIGINT" bash -c "[[ $tunnel_rc -eq 0 ]]"
check "tunnel printed final counters with traffic in both directions" \
    bash -c "grep 'final counters' '$TMP/tunnel.log' | grep -Eq 'android->tun [1-9][0-9]* pkts' && grep 'final counters' '$TMP/tunnel.log' | grep -Eq 'tun->android [1-9][0-9]* pkts'"
check "tunnel printed periodic counters at least once" grep -q ' counters ' "$TMP/tunnel.log"
wait_pid "$FAKE_PID" 3; fake_rc=$WAIT_RC
check "fake Android exited 0 after receiving STOP" bash -c "[[ $fake_rc -eq 0 ]]"
TUNNEL_PID=""; FAKE_PID=""
check "host TUN $HOST_TUN vanished from host-ns (non-persistent)" \
    bash -c "! ip -n $NS_HOST link show $HOST_TUN >/dev/null 2>&1"
check "route $PHONE_IP/32 vanished with the TUN" \
    bash -c "! ip -n $NS_HOST route show $PHONE_IP/32 | grep -q $HOST_TUN"

grep -E 'final counters|session ended' "$TMP/tunnel.log" | sed 's/^/      | /'

# --------------------------------------- abrupt peer loss ("ADB disconnect")
log "phase B: abrupt peer loss under load"
start_tunnel "$TMP/tunnel-b.log"
if [[ -z $HOST_PORT ]]; then fail "tunnel (phase B) did not print HOST_PORT"; exit 1; fi
ip -n $NS_PHONE link del $PHONE_TUN 2>/dev/null || true
start_fake "$TMP/fake-b.log" "$TMP/ready-b"
check "phase B: session re-established" test -f "$TMP/ready-b"
ip -n $NS_HOST route add $PHONE_IP/32 dev $HOST_TUN src $HOST_IP
in_host sysctl -q -w net.ipv4.conf.$HOST_TUN.forwarding=1
check "phase B: ICMP LAN host -> phone" in_lan $PING $PHONE_IP
# Flood from the LAN while the peer disappears without STOP (SIGKILL = adb gone).
ip netns exec $NS_LAN ping -f -s 1000 -w 4 $PHONE_IP >/dev/null 2>&1 &
FLOOD_PID=$!
sleep 0.5
kill -KILL "$FAKE_PID"
wait_pid "$TUNNEL_PID" 5; tunnel_rc=$WAIT_RC
check "phase B: tunnel exited within 5s of the peer vanishing (rc=$tunnel_rc, non-zero expected)" \
    bash -c "[[ $tunnel_rc -ne 255 && $tunnel_rc -ne 0 ]]"
check "phase B: tunnel logged peer loss" grep -Eq 'peer closed connection|transport error' "$TMP/tunnel-b.log"
check "phase B: host TUN $HOST_TUN vanished after abnormal exit" \
    bash -c "! ip -n $NS_HOST link show $HOST_TUN >/dev/null 2>&1"
kill "$FLOOD_PID" 2>/dev/null; wait "$FLOOD_PID" 2>/dev/null
TUNNEL_PID=""; FAKE_PID=""
grep -E 'final counters|session ended' "$TMP/tunnel-b.log" | sed 's/^/      | /'

teardown
snapshot "$TMP/after"
check "root-namespace baseline identical after teardown (routes, links, nft, sysctl, netns, addrs)" \
    diff -ru "$TMP/before" "$TMP/after"
