#!/usr/bin/env bash
# ufw, as Ubuntu ships it, drops routed traffic: doctor must say so with a
# ufw command, the phone must be half-reachable (ping passes, TCP does not)
# until that command runs, and then fully reachable (phone-session.sh).
#
#   PHONE=04e8:6860 ./lab.sh up router ubuntu && ./push.sh ubuntu
#   ./ufw.sh SERIAL
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: ufw.sh SERIAL}
rig_tmp ufw
pc() { timeout 60 "$HERE/lab.sh" ssh ubuntu "$@"; }
router() { timeout 60 "$HERE/lab.sh" ssh router "$@"; }
slowly() { local _; for _ in $(seq 1 30); do "$@" && return 0; sleep 2; done; return 1; }

echo "== ufw's defaults"
pc 'sudo ufw route delete allow in on phone+; sudo ufw route delete allow out on phone+' > /dev/null
pc routedroid doctor > "$S/doctor" 2>&1
check "doctor warns about ufw's FORWARD"  grep -q "^warning   nft ip filter chain FORWARD" "$S/doctor"
check "with the ufw command"              grep -q "let them through: sudo ufw route allow in on phone+" "$S/doctor"
check "and nothing about ip6"             eval "! grep -q 'ip6 filter' '$S/doctor'"
pc routedroid start -s "$SERIAL" --lan-if lan0 --detach > /dev/null
check "connection is active"              slowly eval "pc routedroid status | grep -q ' active '"
IP=$(pc routedroid --json status | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["network"]["phone_ip"])')
check "ping passes ufw"                   router ping -c2 -W2 "$IP"
( pc "adb -s $SERIAL shell 'echo hi | timeout 10 toybox nc -l -p 9000'" > /dev/null 2>&1 & )
sleep 2
check "TCP to the phone does not"         eval "! router 'nc -w4 $IP 9000 </dev/null' | grep -q hi"
pc routedroid stop -s "$SERIAL" > /dev/null
sleep 8  # the phone's listener times out

echo "== doctor's advice, as printed"
ADVICE=$(sed -n 's/.*let them through: //p' "$S/doctor" | head -1)
echo "running: $ADVICE"
pc "$ADVICE" > /dev/null
check "doctor no longer warns"            eval "! pc routedroid doctor | grep -q '^warning   nft'"
echo "== the full session"
check "phone-session.sh passes"           env GUEST=ubuntu "$HERE/phone-session.sh" "$SERIAL"
rig_end
