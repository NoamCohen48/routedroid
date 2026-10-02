#!/usr/bin/env bash
# Leased sessions: the helper leases the phone's address from a real DHCP
# server (dnsmasq) and holds it for the session, unprivileged, in netns.
#
#   ./dhcp-session.sh          # RENEW=0 skips the 70 s renewal check
#
#   host-ns: hv  192.168.70.10/24   helper + stand-in controller
#   lan-ns : lv  192.168.70.1/24    dnsmasq .100-.126, 2 min leases, DNS .53
#            hv2/lv2 192.168.71.0/24  a LAN with no DHCP server
#            198.51.100.1 on lv's lo: "the Internet", behind the LAN's router
#   host-ns: up0 192.168.72.10/24, the host's default route (a dead end)
#
# Checks: a leased start (address, lease, DNS, client-id) without adding an
# address to hv; traffic through the leased address; RELEASE on stop and
# after a helper crash; the renewal, with its unicast ACK kept from the
# phone; a station claiming the address ends the session; a requested
# address in use is refused; DHCP off in the policy, or no server, refuse.
# Egress: with the host's default route on another interface, the leased
# phone still reaches the Internet through the LAN's router, and a phone
# with a requested address and no gateway on its LAN reaches the LAN only;
# losing the LAN interface ends the session and leaves nothing behind.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
BIN=${BIN:-$HERE/../../host/target/release/routedroid-helper}
CLIENT=${CLIENT:-$HERE/../../host/target/release/routedroid-helper-client}
if [[ $(id -u) -ne 0 && ${RD_REEXEC:-0} -ne 1 ]]; then
    export RD_REEXEC=1
    # shellcheck disable=SC2016 # $0 and $@ belong to the inner shell
    exec unshare -Urnm --propagation unchanged bash -c \
        'mount -t tmpfs none /run && mkdir -p /run/netns && exec "$0" "$@"' "$0" "$@"
fi
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
rig_tmp dhcp-session
H=rdhost; L=rdlan; SER=rig-a
ip netns add $H; ip netns add $L
ip -n $H link set lo up; ip -n $L link set lo up
ip link add hv netns $H type veth peer name lv netns $L
ip link add hv2 netns $H type veth peer name lv2 netns $L
ip -n $H addr add 192.168.70.10/24 dev hv; ip -n $L addr add 192.168.70.1/24 dev lv
ip -n $H addr add 192.168.71.10/24 dev hv2; ip -n $L addr add 192.168.71.1/24 dev lv2
ip -n $H link add up0 type dummy; ip -n $H addr add 192.168.72.10/24 dev up0
ip -n $L addr add 198.51.100.1/32 dev lo; ip netns exec $L sysctl -qw net.ipv4.ip_forward=1
for i in hv hv2 up0; do ip -n $H link set $i up; done
ip -n $H route add default via 192.168.72.1 dev up0
for i in lv lv2; do ip -n $L link set $i up; done
# policy true|false: whether phones on hv may lease (hv2 always may).
policy() {
    printf '[[interface]]\nname = "hv"\nphone_addresses = ["192.168.70.96/27"]\ndhcp = %s\n\n[[interface]]\nname = "hv2"\ndhcp = true\n' "$1" > "$S/helper.toml"
}
policy true
ip netns exec $L dnsmasq --no-daemon --interface=lv --bind-interfaces --port=0 \
    --dhcp-range=192.168.70.100,192.168.70.126,2m --dhcp-option=6,192.168.70.53 \
    --dhcp-leasefile="$S/leases" --dhcp-authoritative --log-dhcp --log-facility=- \
    --no-hosts --no-resolv > "$S/dnsmasq.log" 2>&1 &
DPID=$!
in_h() { ip netns exec $H "$@"; }
HELPER=(ip netns exec "$H" "$BIN" --state-dir "$S/state" --policy "$S/helper.toml" --crash-file "$S/crash-at")
serve() { "${HELPER[@]}" serve --socket "$S/helper.sock" >> "$S/helper.log" 2>&1 & HPID=$!; wait_for_socket "$S/helper.sock"; }
trap 'kill $HPID $DPID 2>/dev/null' EXIT
snapshot() { in_h ip -4 -o addr; in_h ip -4 route show table all; in_h nft list tables; in_h cat /proc/sys/net/ipv4/conf/hv/proxy_arp /proc/sys/net/ipv4/conf/hv/forwarding; }
snapshot > "$S/before"
serve
client() { local name=$1; shift; in_h "$CLIENT" --socket "$S/helper.sock" --serial $SER "$@" > "$S/c-$name.log" 2>&1; }
client_bg() { local name=$1; shift; ( exec ip netns exec $H "$CLIENT" --socket "$S/helper.sock" --serial $SER "$@" > "$S/c-$name.log" 2>&1 ) & }
started() { local _; for _ in $(seq 1 100); do grep -q ^STARTED "$S/c-$1.log" && return 0; sleep 0.1; done; return 1; }
field() { sed -n "s/^$2 .*$3=\([^ ]*\).*/\1/p" "$S/c-$1.log" | head -1; }
logged() { grep -q "$1" "$S/dnsmasq.log"; }
# The identity the helper must use: routedroid:<8 bytes of SHA-256(serial)>:<hv MAC>, as dnsmasq writes it.
MAC=$(in_h cat /sys/class/net/hv/address)
CID=$(python3 -c 'import hashlib,sys; h=hashlib.sha256(b"routedroid device id v1\0"+sys.argv[1].encode()).hexdigest()[:16]; c="routedroid:%s:%s" % (h, sys.argv[2].replace(":","")); print(":".join(["00"]+["%02x" % b for b in c.encode()]))' $SER "$MAC")
only_static() { [[ $(in_h ip -4 -o addr show dev hv | wc -l) -eq 1 ]]; }
baseline() { snapshot > "$S/after"; diff "$S/before" "$S/after"; }

echo "== a leased session"
client_bg a --lan-if hv --tun phone0 --hold 300 --bench 20 --bench-target 198.51.100.1; APID=$!
check "started with a lease" started a
IP=$(field a STARTED phone_ip)
check "leased $IP is in the pool"           python3 -c "import ipaddress as i,sys; sys.exit(not i.ip_address('192.168.70.100') <= i.ip_address('$IP') <= i.ip_address('192.168.70.126'))"
check "lease reported: server, router, DNS" grep -q "^LEASE server=192.168.70.1 router=192.168.70.1 dns=192.168.70.53 " "$S/c-a.log"
check "dnsmasq holds it for our client-id"  eventually grep -q " $IP .*$CID" "$S/leases"
check "hv still has only its own address"   only_static
check "/32 route to the phone"              eval "in_h ip -4 route show $IP/32 | grep -q phone0"
check "its egress is the LAN's router"     eval "in_h ip -4 route get 198.51.100.1 from $IP iif phone0 | grep -q 'via 192.168.70.1 dev hv table'"
check "so the Internet answers via the LAN" eventually grep -q "BENCH sent=20 replies=20" "$S/c-a.log"
if [[ ${RENEW:-1} -eq 1 ]]; then
    echo "== renewal at T1 (about 60 s)"
    sleep 70
    check "the controller heard the renewal"    eval "[[ \$(grep -c ^LEASE '$S/c-a.log') -ge 2 ]]"
    check "dnsmasq ACKed the renewal"           eval "[[ \$(grep -c 'DHCPACK(lv) $IP' '$S/dnsmasq.log') -ge 2 ]]"
    check "its ACK never reached the phone"     eval "in_h nft list table inet routedroid_phone0 | grep 'udp sport 67 udp dport 68' | grep -qv 'packets 0 '"
fi
kill -INT $APID; wait $APID
check "stopped" grep -q ^STOPPED "$S/c-a.log"
check "RELEASE reached the server"         eventually logged "DHCPRELEASE(lv) $IP"
check "baseline restored"                  baseline

echo "== another station claims the address"
client_bg b --lan-if hv --tun phone0 --hold 300; BPID=$!
check "B started" started b
IP=$(field b STARTED phone_ip)
LMAC=$(ip netns exec $L cat /sys/class/net/lv/address)
sleep 1.5  # past the announcements
ip netns exec $L python3 - "$IP" "$LMAC" <<'EOF'
import socket, sys
ip, mac = socket.inet_aton(sys.argv[1]), bytes.fromhex(sys.argv[2].replace(":", ""))
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW); s.bind(("lv", 0))
arp = b"\x00\x01\x08\x00\x06\x04\x00\x01" + mac + ip + b"\x00" * 6 + ip
s.send(b"\xff" * 6 + mac + b"\x08\x06" + arp)
EOF
wait $BPID
check "the session ended, saying why"      grep -q "ENDED SessionEnded: .*$LMAC also uses $IP" "$S/c-b.log"
check "the address was declined"           eventually logged "DHCPDECLINE(lv) $IP"
check "baseline restored"                  eventually baseline

echo "== refusals"
ip -n $L addr add 192.168.70.120/32 dev lv
client used --lan-if hv --tun phone1 --phone-ip 192.168.70.120
check "a requested address in use is refused" grep -q "192.168.70.120 is in use on hv: $LMAC answers ARP" "$S/c-used.log"
ip -n $L addr del 192.168.70.120/32 dev lv
client_bg static --lan-if hv --tun phone1 --phone-ip 192.168.70.99 --hold 300; SPID=$!
check "a requested address starts"          started static
check "with no gateway on hv: the LAN only" eval "! in_h ip -4 route get 198.51.100.1 from 192.168.70.99 iif phone1 2>/dev/null"
check "never the host's default route"      eval "in_h ip -4 route get 192.168.70.1 from 192.168.70.99 iif phone1 | grep -q 'dev hv table'"
kill -INT $SPID; wait $SPID
policy false
DISCOVERS=$(grep -c DHCPDISCOVER "$S/dnsmasq.log")
client off --lan-if hv --tun phone1
check "DHCP off in the policy is refused"  grep -q "the policy does not allow DHCP on hv" "$S/c-off.log"
check "without sending anything"           eval "[[ \$(grep -c DHCPDISCOVER '$S/dnsmasq.log') -eq $DISCOVERS ]]"
client none --lan-if hv2 --tun phone1
check "no server: NoLease"                 grep -q "NoLease: no usable DHCP lease on hv2 within 30s" "$S/c-none.log"
check "baseline restored"                  baseline

echo "== a helper crash still gives the lease back"
policy true
echo active > "$S/crash-at"
client crash --lan-if hv --tun phone0 --hold 30
wait $HPID 2>/dev/null
IP=$(sed -n 's/.*session active .*phone=\([0-9.]*\) leased=true.*/\1/p' "$S/helper.log" | tail -1)
check "the helper died with a lease ($IP)"  test -n "$IP"
rm -f "$S/crash-at"
"${HELPER[@]}" cleanup >> "$S/helper.log" 2>&1
check "cleanup RELEASEd it"                eventually logged "DHCPRELEASE(lv) $IP"
check "baseline restored"                  baseline
check "helper check passes"                "${HELPER[@]}" check

echo "== the LAN interface goes away"
serve
client_bg gone --lan-if hv --tun phone0 --hold 300; GPID=$!
check "started" started gone
sleep 1.5  # past the announcements
in_h ip link del hv
wait $GPID
check "the session ended, saying why"      grep -q "^ENDED SessionEnded: " "$S/c-gone.log"
check "no rule or table left"              eval "! in_h ip -4 rule | grep -q 'proto 82' && ! in_h ip -4 route show table all | grep -q 'proto 82'"
check "no TUN left"                        eval "! in_h ip link show phone0 2>/dev/null"
check "helper check passes"                "${HELPER[@]}" check
rig_end
