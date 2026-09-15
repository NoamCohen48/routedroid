#!/usr/bin/env bash
# Phase 0 §3.3 namespace lab: DHCP alias probe (host/phase0-dhcp) vs dnsmasq.
#
#   host-ns: hv  192.168.50.10/24 static ("the PC's own address")   veth
#   lan-ns : lv  192.168.50.1/24 + dnsmasq (range .100-.150, 1h)  <──────>
#            lv.10 192.168.60.1/24 + dnsmasq (VLAN 10, optional check 6)
#
# The client binds an AF_PACKET socket to hv and must obtain, renew, restore
# (INIT-REBOOT) and release extra leases WITHOUT ever adding an address to hv.
#
# Runs as root, or unprivileged: it re-execs itself under
# `unshare -Urnm --propagation unchanged` with a private tmpfs on /run so
# `ip netns` works. Prints PASS/FAIL/SKIP per check, exits non-zero on any FAIL.
set -u -o pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
BIN=${PHASE0_DHCP:-$REPO/host/target/release/phase0-dhcp}
[[ -x $BIN ]] || BIN=$REPO/host/target/debug/phase0-dhcp

# ------------------------------------------------------ unprivileged re-exec
if [[ $(id -u) -ne 0 ]]; then
    if [[ ${RD_DHCP_LAB_REEXEC:-0} -eq 1 ]]; then
        echo "re-exec under unshare -Urnm did not yield uid 0; user namespaces disabled?"; exit 2
    fi
    command -v unshare >/dev/null || { echo "not root and no unshare(1); run with sudo"; exit 2; }
    echo "[lab] not root: re-executing in an unprivileged user+net+mount namespace"
    export RD_DHCP_LAB_REEXEC=1
    exec unshare -Urnm --propagation unchanged bash -c \
        'mount -t tmpfs none /run && mkdir -p /run/netns && exec "$0" "$@"' "$0" "$@"
fi
IN_USERNS=${RD_DHCP_LAB_REEXEC:-0}

NS_HOST=rd3host
NS_LAN=rd3lan
VETH_HOST=hv
VETH_LAN=lv
HOST_IP=192.168.50.10
LAN_IP=192.168.50.1
RANGE_LO=192.168.50.100
RANGE_HI=192.168.50.150
VLAN_ID=10
VLAN_LAN_IP=192.168.60.1
VLAN_HOST_IP=192.168.60.10
CID_A="routedroid:lab:$$:a"
CID_B="routedroid:lab:$$:b"
CID_C="routedroid:lab:$$:c"
CID_V="routedroid:lab:$$:vlan"

TMP=$(mktemp -d /tmp/rd3-lab.XXXXXX)
FAILS=0
PASSES=0
SKIPS=0
DNSMASQ_PID=""
DNSMASQ_VLAN_PID=""
SNIFF_PID=""

log()  { printf '\033[1;34m[lab]\033[0m %s\n' "$*"; }
pass() { printf '\033[1;32mPASS\033[0m  %s\n' "$*"; PASSES=$((PASSES + 1)); }
fail() { printf '\033[1;31mFAIL\033[0m  %s\n' "$*"; FAILS=$((FAILS + 1)); }
skip() { printf '\033[1;33mSKIP\033[0m  %s\n' "$*"; SKIPS=$((SKIPS + 1)); }
check() { # check "name" cmd...
    local name=$1; shift
    if "$@" >"$TMP/check.out" 2>&1; then pass "$name"; else fail "$name"; sed 's/^/      | /' "$TMP/check.out" | tail -n 12; fi
}
in_host() { ip netns exec "$NS_HOST" "$@"; }
in_lan()  { ip netns exec "$NS_LAN" "$@"; }
# dhcp <logfile> <args...>: run the probe in host-ns, capture stderr, echo stdout JSON; returns its exit code.
dhcp() { local logf=$1; shift; ip netns exec "$NS_HOST" "$BIN" "$@" 2>"$logf"; }
json() { python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d[sys.argv[2]])' "$1" "$2"; }
in_range() { python3 -c 'import ipaddress,sys; a=ipaddress.ip_address(sys.argv[1]); sys.exit(0 if ipaddress.ip_address(sys.argv[2])<=a<=ipaddress.ip_address(sys.argv[3]) else 1)' "$1" "$2" "$3"; }
host_v4_addrs() { ip -n "$NS_HOST" -4 -o addr show dev "$1" | awk '{print $4}' | sort | tr '\n' ' '; }
# dnsmasq stores a type-0 client-id as "00:<hex bytes>" in its lease file.
cid_hex() { python3 -c 'import sys; print(":".join(["00"] + ["%02x" % b for b in sys.argv[1].encode()]))' "$1"; }
export -f json in_range host_v4_addrs cid_hex
export NS_HOST
HEX_A=$(cid_hex "$CID_A"); HEX_B=$(cid_hex "$CID_B"); HEX_C=$(cid_hex "$CID_C"); HEX_V=$(cid_hex "$CID_V")

# ------------------------------------------------------------- preconditions
for t in ip python3 dnsmasq; do command -v "$t" >/dev/null || { echo "missing tool: $t"; exit 2; }; done
if [[ ! -x $BIN ]]; then
    echo "phase0-dhcp binary not found at $BIN; build it first: (cd $REPO/host && cargo build --release)"; exit 2
fi
for ns in $NS_HOST $NS_LAN; do
    if ip netns list 2>/dev/null | grep -qw "$ns"; then echo "namespace $ns already exists; refusing to run"; exit 2; fi
done

snapshot() { # snapshot <dir>  (root-namespace baseline; trivial inside the throwaway userns)
    local d=$1; mkdir -p "$d"
    ip -o link show | awk -F': ' '{print $2}' | sort >"$d/links"
    ip -o addr show | awk '{print $2, $4}' | sort >"$d/addrs"
    ip -4 route show table all >"$d/routes"
    ip netns list 2>/dev/null | sort >"$d/netns"
}
snapshot "$TMP/before"

# ------------------------------------------------------------------- cleanup
teardown_done=0
teardown() {
    [[ $teardown_done -eq 1 ]] && return
    teardown_done=1
    log "teardown"
    for p in "$SNIFF_PID" "$DNSMASQ_PID" "$DNSMASQ_VLAN_PID"; do
        [[ -n $p ]] && kill "$p" 2>/dev/null
    done
    wait 2>/dev/null
    for ns in $NS_HOST $NS_LAN; do ip netns del "$ns" 2>/dev/null; done
}
on_exit() {
    local rc=$?
    teardown
    if [[ $rc -ne 0 && $FAILS -eq 0 ]]; then fail "script aborted (rc=$rc)"; fi
    [[ ${KEEP_TMP:-0} -eq 1 ]] && log "kept $TMP" || rm -rf "$TMP"
    if [[ $FAILS -eq 0 ]]; then log "ALL $PASSES CHECKS PASSED ($SKIPS skipped)"; exit 0; else log "$FAILS FAILED, $PASSES passed, $SKIPS skipped"; exit 1; fi
}
trap on_exit EXIT
trap 'exit 130' INT TERM

# ---------------------------------------------------------------- namespaces
log "building namespaces (userns=$IN_USERNS)"
ip netns add $NS_HOST
ip netns add $NS_LAN
ip -n $NS_HOST link set lo up
ip -n $NS_LAN link set lo up
ip link add $VETH_HOST netns $NS_HOST type veth peer name $VETH_LAN netns $NS_LAN
ip -n $NS_HOST addr add $HOST_IP/24 dev $VETH_HOST
ip -n $NS_HOST link set $VETH_HOST up
ip -n $NS_LAN addr add $LAN_IP/24 dev $VETH_LAN
ip -n $NS_LAN link set $VETH_LAN up
HOST_MAC=$(ip -n $NS_HOST -o link show $VETH_HOST | sed -n 's/.*link\/ether \([0-9a-f:]*\).*/\1/p')
LAN_MAC=$(ip -n $NS_LAN -o link show $VETH_LAN | sed -n 's/.*link\/ether \([0-9a-f:]*\).*/\1/p')
log "host $VETH_HOST $HOST_MAC $HOST_IP ; lan $VETH_LAN $LAN_MAC $LAN_IP"

# --------------------------------------------------------- sniffer in lan-ns
# Records every DHCP frame seen on an interface in lan-ns, with the Ethernet
# addresses and the VLAN tag reported through PACKET_AUXDATA, so the checks can
# prove unicast vs broadcast on the wire independently of the client's logs.
cat >"$TMP/sniff.py" <<'EOF'
import socket, struct, sys, time
ifname, out = sys.argv[1], sys.argv[2]
SOL_PACKET, PACKET_AUXDATA = 263, 8
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3))
s.setsockopt(SOL_PACKET, PACKET_AUXDATA, 1)
s.bind((ifname, 0))
f = open(out, "a", buffering=1)
f.write("# ready\n")
while True:
    data, anc, flags, addr = s.recvmsg(65535, 256)
    vlan = "-"
    for lvl, typ, cd in anc:
        if lvl == SOL_PACKET and typ == PACKET_AUXDATA and len(cd) >= 20:
            st, ln, sn, mac, net, tci, tpid = struct.unpack_from("IIIHHHH", cd)
            if st & (1 << 4):
                vlan = str(tci & 0xfff)
    if len(data) < 42 or data[12:14] != b"\x08\x00" or data[23] != 17:
        continue
    ihl = (data[14] & 0xf) * 4
    u = 14 + ihl
    sp, dp = struct.unpack("!HH", data[u:u + 4])
    if 67 not in (sp, dp):
        continue
    b = data[u + 8:]
    if len(b) < 240:
        continue
    mt, xid = 0, struct.unpack("!I", b[4:8])[0]
    ciaddr = socket.inet_ntoa(b[12:16])
    i = 240
    while i + 1 < len(b) and b[i] != 255:
        if b[i] == 0:
            i += 1; continue
        c, l = b[i], b[i + 1]
        if c == 53 and l == 1 and i + 2 < len(b):
            mt = b[i + 2]
        i += 2 + l
    mac = lambda x: ":".join("%02x" % c for c in x)
    f.write("%s dir=%s dst=%s src=%s ip=%s>%s port=%d>%d type=%d ciaddr=%s xid=%08x vlan=%s\n" % (
        time.strftime("%H:%M:%S"), "out" if addr[2] == 4 else "in", mac(data[0:6]), mac(data[6:12]),
        socket.inet_ntoa(data[26:30]), socket.inet_ntoa(data[30:34]), sp, dp, mt, ciaddr, xid, vlan))
EOF
ip netns exec $NS_LAN python3 "$TMP/sniff.py" $VETH_LAN "$TMP/sniff.log" &
SNIFF_PID=$!
for _ in $(seq 1 30); do grep -q '# ready' "$TMP/sniff.log" 2>/dev/null && break; sleep 0.1; done

# ------------------------------------------------------------------ dnsmasq
start_dnsmasq() { # start_dnsmasq <iface> <range-lo> <range-hi> <leasefile> <logfile>; echoes pid
    ip netns exec $NS_LAN dnsmasq --no-daemon --interface="$1" --bind-interfaces \
        --dhcp-range="$2,$3,1h" --dhcp-leasefile="$4" --log-dhcp --port=0 \
        --dhcp-authoritative --log-facility=- --no-hosts --no-resolv >"$5" 2>&1 &
    echo $!
}
LEASES=$TMP/leases
: >"$LEASES"
DNSMASQ_PID=$(start_dnsmasq $VETH_LAN $RANGE_LO $RANGE_HI "$LEASES" "$TMP/dnsmasq.log")
for _ in $(seq 1 50); do grep -q 'DHCP, sockets bound' "$TMP/dnsmasq.log" 2>/dev/null && break; sleep 0.1; done
if ! kill -0 "$DNSMASQ_PID" 2>/dev/null || ! grep -q 'DHCP, sockets bound' "$TMP/dnsmasq.log"; then
    fail "dnsmasq did not start"; cat "$TMP/dnsmasq.log"; exit 1
fi
pass "dnsmasq serving $RANGE_LO-$RANGE_HI on $VETH_LAN in lan-ns (pid $DNSMASQ_PID)"
BASE_ADDRS=$(host_v4_addrs $VETH_HOST)
check "host-ns $VETH_HOST has exactly one IPv4 address before the probe ($BASE_ADDRS)" \
    bash -c "[[ '$BASE_ADDRS' == '$HOST_IP/24 ' ]]"

# ------------------------------------------------------- 1. acquire (no hold)
log "check 1: acquire for $CID_A"
dhcp "$TMP/a.log" acquire --iface $VETH_HOST --client-id "$CID_A" --state "$TMP/a.json" --timeout 30 >"$TMP/a.out"
rc=$?
check "1. acquire exited 0 and wrote a lease record (rc=$rc)" bash -c "[[ $rc -eq 0 && -s $TMP/a.json ]]"
if [[ -s $TMP/a.json ]]; then
    A_IP=$(json "$TMP/a.json" address)
    A_SRV=$(json "$TMP/a.json" server_id)
    A_SMAC=$(json "$TMP/a.json" server_mac)
    check "1. lease $A_IP is inside $RANGE_LO-$RANGE_HI" in_range "$A_IP" $RANGE_LO $RANGE_HI
    check "1. lease record: prefix 24, router $LAN_IP, server_id $LAN_IP, server_mac = lan veth MAC, lease 3600" \
        bash -c "[[ \$(json $TMP/a.json prefix) == 24 && \$(json $TMP/a.json router) == $LAN_IP && '$A_SRV' == $LAN_IP && '$A_SMAC' == $LAN_MAC && \$(json $TMP/a.json lease_secs) == 3600 ]]"
    check "1. stdout carried the same JSON record" bash -c "grep -q '\"address\":\"$A_IP\"' $TMP/a.out"
    check "1. dnsmasq lease file lists $A_IP with client-id $CID_A" \
        bash -c "sleep 0.5; grep -q ' $A_IP .*$HEX_A' $LEASES"
    check "1. DISCOVER and REQUEST were broadcast with the broadcast flag; OFFER/ACK came back broadcast" \
        bash -c "grep -q 'dst=ff:ff:ff:ff:ff:ff src=$HOST_MAC ip=0.0.0.0>255.255.255.255 port=68>67 type=1' $TMP/sniff.log && grep -q 'dst=ff:ff:ff:ff:ff:ff src=$HOST_MAC ip=0.0.0.0>255.255.255.255 port=68>67 type=3' $TMP/sniff.log && grep -q 'dir=out dst=ff:ff:ff:ff:ff:ff .*type=5' $TMP/sniff.log"
    check "1. client log: chaddr/XID validated ACK, address NOT configured" grep -q 'BOUND (address NOT configured' "$TMP/a.log"
else
    A_IP=""; sed 's/^/      | /' "$TMP/a.log" | tail -20
fi
NOW_ADDRS=$(host_v4_addrs $VETH_HOST)
check "1. host-ns $VETH_HOST still has only $HOST_IP/24 (ip -4 addr: $NOW_ADDRS)" \
    bash -c "[[ '$NOW_ADDRS' == '$HOST_IP/24 ' ]]"
check "1. host-ns has no address in 192.168.50.100-150 on any interface" \
    bash -c "! ip -n $NS_HOST -4 -o addr show | grep -Eq '192\.168\.50\.1[0-4][0-9]|192\.168\.50\.150'"

# --------------------------------------------- 2. two identities concurrently
log "check 2: concurrent acquire for $CID_B and $CID_C"
dhcp "$TMP/b.log" acquire --iface $VETH_HOST --client-id "$CID_B" --state "$TMP/b.json" --timeout 30 >"$TMP/b.out" &
PB=$!
dhcp "$TMP/c.log" acquire --iface $VETH_HOST --client-id "$CID_C" --state "$TMP/c.json" --timeout 30 >"$TMP/c.out" &
PC=$!
wait $PB; rcb=$?
wait $PC; rcc=$?
check "2. both concurrent acquires exited 0 (rc=$rcb,$rcc)" bash -c "[[ $rcb -eq 0 && $rcc -eq 0 && -s $TMP/b.json && -s $TMP/c.json ]]"
if [[ -s $TMP/b.json && -s $TMP/c.json ]]; then
    B_IP=$(json "$TMP/b.json" address); C_IP=$(json "$TMP/c.json" address)
    check "2. leases differ: A=$A_IP B=$B_IP C=$C_IP" \
        bash -c "[[ -n '$A_IP' && '$A_IP' != '$B_IP' && '$A_IP' != '$C_IP' && '$B_IP' != '$C_IP' ]]"
    check "2. both in range" bash -c "in_range $B_IP $RANGE_LO $RANGE_HI && in_range $C_IP $RANGE_LO $RANGE_HI"
    check "2. dnsmasq lease file has all three identities" \
        bash -c "sleep 0.5; grep -q ' $B_IP .*$HEX_B' $LEASES && grep -q ' $C_IP .*$HEX_C' $LEASES && grep -q ' $A_IP .*$HEX_A' $LEASES"
    check "2. each client ignored the other's replies (foreign xid) and none saw a mismatched ACK" \
        bash -c "! grep -q 'server changed our address' $TMP/b.log $TMP/c.log"
else
    B_IP=""; C_IP=""; sed 's/^/      | /' "$TMP/b.log" "$TMP/c.log" | tail -20
fi
check "2. host-ns $VETH_HOST still has only $HOST_IP/24" bash -c "[[ '$(host_v4_addrs $VETH_HOST)' == '$HOST_IP/24 ' ]]"

# ------------------------------------------------------- 3. renew from state
log "check 3: renew $CID_A from state"
if [[ -n $A_IP ]]; then
    MARK=$(wc -l <"$TMP/sniff.log")
    DMARK=$(wc -l <"$TMP/dnsmasq.log")
    dhcp "$TMP/renew.log" renew --iface $VETH_HOST --state "$TMP/a.json" --timeout 10 >"$TMP/renew.out"
    rc=$?
    check "3. renew exited 0 (rc=$rc)" bash -c "[[ $rc -eq 0 ]]"
    check "3. renew ACK kept the address $A_IP" bash -c "[[ \$(json $TMP/a.json address) == $A_IP ]] && grep -q '\"address\":\"$A_IP\"' $TMP/renew.out"
    tail -n +"$((MARK + 1))" "$TMP/sniff.log" >"$TMP/sniff.renew"
    check "3. on the wire: REQUEST was UNICAST (eth dst $LAN_MAC, ip $A_IP>$LAN_IP, ciaddr $A_IP)" \
        grep -q "dir=in dst=$LAN_MAC src=$HOST_MAC ip=$A_IP>$LAN_IP port=68>67 type=3 ciaddr=$A_IP" "$TMP/sniff.renew"
    check "3. on the wire: ACK was UNICAST back to $A_IP at $HOST_MAC (server resolved it via the client's ARP responder)" \
        grep -q "dir=out dst=$HOST_MAC src=$LAN_MAC ip=$LAN_IP>$A_IP port=67>68 type=5" "$TMP/sniff.renew"
    tail -n +"$((DMARK + 1))" "$TMP/dnsmasq.log" >"$TMP/dnsmasq.renew"
    check "3. dnsmasq log: DHCPREQUEST/DHCPACK for $A_IP without 'broadcast response'" \
        bash -c "grep -q 'DHCPREQUEST($VETH_LAN) $A_IP' $TMP/dnsmasq.renew && grep -q 'DHCPACK($VETH_LAN) $A_IP' $TMP/dnsmasq.renew && ! grep -q 'broadcast response' $TMP/dnsmasq.renew"
    check "3. client log shows the unicast RENEW and the ARP reply" \
        bash -c "grep -q 'RENEWING: unicast REQUEST' $TMP/renew.log && grep -q 'rx kind=ACK' $TMP/renew.log && grep -q 'ARP responder: answering' $TMP/renew.log"
    check "3. host-ns $VETH_HOST still has only $HOST_IP/24" bash -c "[[ '$(host_v4_addrs $VETH_HOST)' == '$HOST_IP/24 ' ]]"
else
    fail "3. skipped: no lease from check 1"
fi

# ----------------------------------------------------------- 4. init-reboot
log "check 4: init-reboot"
if [[ -n $A_IP ]]; then
    MARK=$(wc -l <"$TMP/sniff.log")
    dhcp "$TMP/ir.log" init-reboot --iface $VETH_HOST --state "$TMP/a.json" --timeout 10 >"$TMP/ir.out"
    rc=$?
    check "4. init-reboot with valid state -> ACK, exit 0 (rc=$rc), same address" \
        bash -c "[[ $rc -eq 0 && \$(json $TMP/a.json address) == $A_IP ]]"
    tail -n +"$((MARK + 1))" "$TMP/sniff.log" >"$TMP/sniff.ir"
    check "4. on the wire: broadcast REQUEST from 0.0.0.0 with ciaddr 0 (option 50, no 54)" \
        grep -q "dst=ff:ff:ff:ff:ff:ff src=$HOST_MAC ip=0.0.0.0>255.255.255.255 port=68>67 type=3 ciaddr=0.0.0.0" "$TMP/sniff.ir"
    python3 - "$TMP/a.json" "$TMP/bad.json" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1])); d["address"] = "192.168.50.200"; json.dump(d, open(sys.argv[2], "w"))
EOF
    dhcp "$TMP/ir-bad.log" init-reboot --iface $VETH_HOST --state "$TMP/bad.json" --timeout 10 >"$TMP/ir-bad.out"
    rc=$?
    check "4. init-reboot for 192.168.50.200 -> NAK, exit 3 (rc=$rc)" bash -c "[[ $rc -eq 3 ]] && grep -q 'INIT-REBOOT: NAK' $TMP/ir-bad.log"
    check "4. dnsmasq logged the DHCPNAK" grep -q 'DHCPNAK.*192.168.50.200' "$TMP/dnsmasq.log"
    check "4. NAK did not disturb the real lease ($A_IP still in lease file)" bash -c "grep -q ' $A_IP .*$HEX_A' $LEASES"
else
    fail "4. skipped: no lease from check 1"
fi

# ----------------------------------------------------------------- 5. release
log "check 5: release"
if [[ -n $A_IP ]]; then
    MARK=$(wc -l <"$TMP/sniff.log")
    dhcp "$TMP/rel.log" release --iface $VETH_HOST --state "$TMP/a.json"
    rc=$?
    check "5. release exited 0 (rc=$rc)" bash -c "[[ $rc -eq 0 ]]"
    tail -n +"$((MARK + 1))" "$TMP/sniff.log" >"$TMP/sniff.rel"
    check "5. on the wire: RELEASE was UNICAST (eth dst $LAN_MAC, ip $A_IP>$LAN_IP, ciaddr $A_IP)" \
        grep -q "dir=in dst=$LAN_MAC src=$HOST_MAC ip=$A_IP>$LAN_IP port=68>67 type=7 ciaddr=$A_IP" "$TMP/sniff.rel"
    check "5. dnsmasq logged DHCPRELEASE for $A_IP" bash -c "sleep 0.5; grep -q 'DHCPRELEASE($VETH_LAN) $A_IP' $TMP/dnsmasq.log"
    check "5. $A_IP is gone from the dnsmasq lease file (B and C remain)" \
        bash -c "for i in 1 2 3 4 5 6 7 8 9 10; do grep -q ' $A_IP ' $LEASES || break; sleep 0.5; done; ! grep -q ' $A_IP ' $LEASES && grep -q ' $B_IP ' $LEASES && grep -q ' $C_IP ' $LEASES"
    # Tidy the concurrent leases too (best effort, also exercises release again).
    dhcp "$TMP/rel-b.log" release --iface $VETH_HOST --state "$TMP/b.json"
    dhcp "$TMP/rel-c.log" release --iface $VETH_HOST --state "$TMP/c.json"
    check "5. releasing B and C empties the lease file" \
        bash -c "for i in 1 2 3 4 5 6 7 8 9 10; do [[ ! -s $LEASES ]] && break; sleep 0.5; done; [[ ! -s $LEASES ]]"
else
    fail "5. skipped: no lease from check 1"
fi

# -------------------------------------------------------------------- 6. VLAN
log "check 6: VLAN netdevice"
vlan_ok=0
if [[ $IN_USERNS -eq 0 ]] && ! grep -q '^8021q ' /proc/modules 2>/dev/null; then modprobe 8021q 2>/dev/null || true; fi
if ip -n $NS_LAN link add link $VETH_LAN name $VETH_LAN.$VLAN_ID type vlan id $VLAN_ID 2>"$TMP/vlan.err" \
   && ip -n $NS_HOST link add link $VETH_HOST name $VETH_HOST.$VLAN_ID type vlan id $VLAN_ID 2>>"$TMP/vlan.err"; then
    vlan_ok=1
else
    if [[ $IN_USERNS -eq 1 ]]; then
        skip "6. VLAN: cannot create 802.1Q netdevices here ($(tr '\n' ' ' <"$TMP/vlan.err")); the 8021q module must already be loaded (sudo modprobe 8021q) or run the lab as root"
    else
        fail "6. VLAN: cannot create 802.1Q netdevices as root ($(tr '\n' ' ' <"$TMP/vlan.err"))"
    fi
fi
if [[ $vlan_ok -eq 1 ]]; then
    ip -n $NS_LAN addr add $VLAN_LAN_IP/24 dev $VETH_LAN.$VLAN_ID
    ip -n $NS_LAN link set $VETH_LAN.$VLAN_ID up
    ip -n $NS_HOST addr add $VLAN_HOST_IP/24 dev $VETH_HOST.$VLAN_ID
    ip -n $NS_HOST link set $VETH_HOST.$VLAN_ID up
    VLEASES=$TMP/leases-vlan; : >"$VLEASES"
    DNSMASQ_VLAN_PID=$(start_dnsmasq $VETH_LAN.$VLAN_ID 192.168.60.100 192.168.60.150 "$VLEASES" "$TMP/dnsmasq-vlan.log")
    for _ in $(seq 1 50); do grep -q 'DHCP, sockets bound' "$TMP/dnsmasq-vlan.log" 2>/dev/null && break; sleep 0.1; done
    MARK=$(wc -l <"$TMP/sniff.log")
    dhcp "$TMP/v.log" acquire --iface $VETH_HOST.$VLAN_ID --client-id "$CID_V" --state "$TMP/v.json" --timeout 30 --release-on-exit >"$TMP/v.out"
    rc=$?
    check "6. acquire on VLAN netdevice $VETH_HOST.$VLAN_ID exited 0 (rc=$rc)" bash -c "[[ $rc -eq 0 && -s $TMP/v.json ]]"
    if [[ -s $TMP/v.json ]]; then
        V_IP=$(json "$TMP/v.json" address)
        check "6. VLAN lease $V_IP is in 192.168.60.100-150 (served only on VLAN $VLAN_ID)" in_range "$V_IP" 192.168.60.100 192.168.60.150
        tail -n +"$((MARK + 1))" "$TMP/sniff.log" >"$TMP/sniff.vlan"
        check "6. parent-interface sniffer saw the DISCOVER tagged VLAN $VLAN_ID via PACKET_AUXDATA" \
            grep -q "port=68>67 type=1 .*vlan=$VLAN_ID" "$TMP/sniff.vlan"
        check "6. client bound to the VLAN netdevice saw untagged frames (kernel strips the tag below the VLAN device)" \
            bash -c "grep -q 'PACKET_AUXDATA: no VLAN tag' $TMP/v.log && [[ \$(json $TMP/v.json vlan_tagged_replies) == False ]]"
        check "6. VLAN netdevice kept only its static address ($(host_v4_addrs $VETH_HOST.$VLAN_ID))" \
            bash -c "[[ '$(host_v4_addrs $VETH_HOST.$VLAN_ID)' == '$VLAN_HOST_IP/24 ' ]]"
        check "6. untagged dnsmasq never saw the VLAN client ($CID_V absent from its lease file)" bash -c "! grep -q '$HEX_V' $LEASES"
    else
        sed 's/^/      | /' "$TMP/v.log" | tail -20
    fi
fi

# ----------------------------------------------------------------- baseline
teardown
snapshot "$TMP/after"
check "root-namespace baseline identical after teardown (links, addrs, routes, netns)" diff -ru "$TMP/before" "$TMP/after"
