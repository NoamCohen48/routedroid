#!/usr/bin/env bash
# Pull the phone's cable mid-session (qemu drops the USB device from the
# guest) and put it back: the connection must hold the phone's address,
# lease and host side while it is away, resume on the same helper session
# when it is back, and carry traffic again. The app closing the connection
# with the phone still attached ends it, and so does a phone that stays
# away past --reconnect-wait, released and clean.
#
#   PHONE=04e8:6860 ./lab.sh up router ubuntu && ./push.sh ubuntu
#   PHONE=04e8:6860 ./unplug.sh SERIAL
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: PHONE=vendor:product unplug.sh SERIAL}
: "${PHONE:?set PHONE=vendor:product (lsusb)}"
rig_tmp unplug
pc() { timeout 60 "$HERE/lab.sh" ssh ubuntu "$@"; }
router() { timeout 60 "$HERE/lab.sh" ssh router "$@"; }
slowly() { local _; for _ in $(seq 1 30); do "$@" && return 0; sleep 2; done; return 1; }
state() { pc routedroid status | grep -q " $1 "; }
sessions() { pc "journalctl --user -u routedroid --since @$SINCE --no-pager | grep -c 'helper session started'"; }
phone_ip() { pc routedroid --json status | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["network"]["phone_ip"])'; }
cable() { "$HERE/lab.sh" "$1" ubuntu > /dev/null; }
held() { pc "ip -4 rule | grep -q 'from $IP lookup' && ip link show phone0 > /dev/null"; }
same_address() { [ "$(phone_ip)" = "$IP" ]; }
one_session() { [ "$(sessions)" = 1 ]; }
reaches() { router ping -c2 -W2 "$IP" > /dev/null; }

pc "bash -s $SERIAL" < "$HERE/../emulator/prepare-device.sh"
pc 'printf "[[interface]]\nname = \"lan0\"\ndhcp = true\n" | sudo tee /etc/routedroid/helper.toml >/dev/null'
SINCE=$(pc date +%s)
echo "== unplugged and back"
pc routedroid start -s "$SERIAL" --lan-if lan0 --detach > /dev/null
check "connection is active"             slowly state active
IP=$(phone_ip)
echo "phone is $IP"
check "the LAN reaches it"               reaches
cable unplug
check "unplugged, it is reconnecting"    slowly state reconnecting
check "its address and host side held"   held
cable plug
check "back, it is active again"         slowly state active
check "with the same address"            same_address
check "on the same helper session"       one_session
check "and the LAN reaches it again"     slowly reaches
check "the phone reaches the router"     eval "pc \"adb -s $SERIAL shell ping -c2 -W3 192.168.80.1\" | grep -q ' 0% packet loss'"
check "stop"                             pc routedroid stop -s "$SERIAL"
echo "== the app closes it, the phone stays"
pc routedroid start -s "$SERIAL" --lan-if lan0 --detach > /dev/null
check "connection is active"             slowly state active
pc "adb -s $SERIAL shell am force-stop dev.routedroid"
check "it ends rather than waiting"      slowly eval "pc routedroid status | grep -q 'no connections'"
check "as the phone closing it"          eval "pc journalctl --user -u routedroid --since -1min --no-pager | grep -q 'ended: the phone closed the connection'"
echo "== away for too long"
pc routedroid start -s "$SERIAL" --lan-if lan0 --reconnect-wait 15s --detach > /dev/null
check "connection is active"             slowly state active
IP=$(phone_ip)
cable unplug
check "unplugged, it is reconnecting"    slowly state reconnecting
check "after the wait, it has ended"     slowly eval "pc routedroid status | grep -q 'no connections'"
check "saying why"                       eval "pc journalctl --user -u routedroid --since -1min --no-pager | grep -q 'not back within 15 s'"
check "the lease was released"           slowly router "sudo journalctl -u dnsmasq --since -2min --no-pager | grep -q 'DHCPRELEASE(lan0) $IP'"
check "nothing left behind"              eval "pc \"! ip -4 rule | grep -q 'proto 82' && ! ip link show phone0 2>/dev/null\""
cable plug
check "the phone is back for the next"   slowly eval "pc adb devices | grep -q '$SERIAL.device'"
rig_end
