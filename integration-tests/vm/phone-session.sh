#!/usr/bin/env bash
# A real phone on a real (virtual) LAN, through the installed units: the
# `host` guest runs routedroidd and the root helper, the `router` guest is
# the LAN's DHCP server, gateway and the peer that reaches the phone.
#
#   ./lab.sh up router && PHONE=04e8:6860 ./lab.sh up host && ./push.sh
#   ./phone-session.sh SERIAL
#
# Checks: doctor passes; a leased start; dnsmasq holds the lease; the phone's
# egress rule and table; the router pings the phone and connects to it over
# TCP; the phone reaches the router, the Internet (from its own address, seen
# on the LAN: no NAT on the host) and resolves names; stop releases the lease
# and leaves nothing behind.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: phone-session.sh SERIAL}
rig_tmp phone-session
host() { timeout 60 "$HERE/lab.sh" ssh host "$@"; }
router() { timeout 60 "$HERE/lab.sh" ssh router "$@"; }
# ssh joins its arguments into one remote command line: quote them for it,
# so a pipe in "$@" runs on the phone, not on the guest.
phone() { host "adb -s $SERIAL shell $(printf '%q ' "$*")"; }

host "bash -s $SERIAL" < "$HERE/../emulator/prepare-device.sh"
host 'printf "[[interface]]\nname = \"lan0\"\ndhcp = true\n" | sudo tee /etc/routedroid/helper.toml >/dev/null'
check "doctor passes"                   host routedroid doctor
echo "== a leased session"
host routedroid start -s "$SERIAL" --lan-if lan0 --detach
# A real phone takes seconds, not lib.sh's eventually: up to 60 s.
slowly() { local _; for _ in $(seq 1 30); do "$@" && return 0; sleep 2; done; return 1; }
active() { host routedroid status | grep -q " active "; }
check "connection is active"            slowly active
IP=$(host routedroid --json status | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["network"]["phone_ip"])')
echo "phone is $IP"
check "status says leased"              eval "host routedroid status | grep -q 'leased, '"
# The client-id starts "routedroid:" (hex 72:6f:75...) after dnsmasq's 00 type byte.
check "dnsmasq leased it to Routedroid" router "grep -q ' $IP .* 00:72:6f:75:74:65:64:72:6f:69:64' /var/lib/misc/dnsmasq.leases"
check "egress rule for the phone"       eval "host ip -4 rule | grep -q 'from $IP lookup .* proto 82'"
check "egress via the LAN's router"     eval "host sudo ip -4 route get 1.1.1.1 from $IP iif phone0 | grep -q 'via 192.168.80.1 dev lan0'"
echo "== the LAN reaches the phone"
check "router pings the phone"          router ping -c3 -W2 "$IP"
( phone "echo hello-from-phone | timeout 15 toybox nc -l -p 9000" > /dev/null 2>&1 & )
sleep 2
check "router opens TCP to the phone"   eval "router 'nc -w5 $IP 9000 </dev/null' | grep -q hello-from-phone"
echo "== the phone reaches out"
check "phone pings the router"          eval "phone ping -c2 -W3 192.168.80.1 | grep -q ' 0% packet loss'"
router "sudo timeout 10 tcpdump -lni lan0 -c 2 'icmp and host 1.1.1.1' 2>/dev/null" > "$S/capture" &
CAP=$!; sleep 2
check "phone pings the Internet"        eval "phone ping -c2 -W3 1.1.1.1 | grep -q ' 0% packet loss'"
wait $CAP
check "with its own address on the LAN" grep -q "IP $IP > 1.1.1.1" "$S/capture"
check "phone resolves names"            eval "phone ping -c1 -W3 deb.debian.org | grep -q '1 received'"
echo "== stop"
check "stop"                            host routedroid stop -s "$SERIAL"
check "the lease was released"          slowly eval "router sudo journalctl -u dnsmasq --no-pager | grep -q 'DHCPRELEASE(lan0) $IP'"
check "no egress rule left"             eval "! host ip -4 rule | grep -q 'proto 82'"
check "no nft table left"               eval "! host sudo nft list tables | grep -q routedroid"
check "doctor finds nothing"            host routedroid doctor
rig_end
