#!/usr/bin/env bash
# Phase 0 §3.2: multiple Android addresses on one VpnService interface.
# Real USB phone, unprivileged user namespace on the host (no root needed).
#
#   ./phone-multialias.sh SERIAL 32     # aliases as /32 + explicit LAN routes
#   ./phone-multialias.sh SERIAL 24     # aliases with their actual LAN prefix
#
# Host side of phone0 gets three "LANs": office 10.77.0.0/24, lab 10.78.0.0/24,
# and a stand-in for the Internet 10.99.0.0/24 (reached via the default route).
# Phone gets 10.77.0.2 (primary) and 10.78.0.2. We record which source the
# phone chooses for each destination, plus its LinkProperties/route tables.
set -euo pipefail
SERIAL=${1:?serial}; PFX=${2:?32 or 24}
HERE=$(cd "$(dirname "$0")" && pwd)
BIN=${BIN:-$HERE/../../host/target/release/phase0-tunnel}
S=$(mktemp -d /tmp/rd-ma.XXXXXX)
OFFICE=10.77.0; LAB=10.78.0; INET=10.99.0
SESSION=p0-ma-$(head -c4 /dev/urandom | xxd -p)
SECRET=$S/secret; head -c32 /dev/urandom | xxd -p -c64 > "$SECRET"
log() { printf '\n== %s\n' "$*"; }
pass=0; fail=0
check() { local name=$1; shift; if "$@"; then echo "PASS  $name"; pass=$((pass+1)); else echo "FAIL  $name"; fail=$((fail+1)); fi; }

cleanup() {
    set +e
    [[ -n ${TPID:-} ]] && kill -INT "$TPID" 2>/dev/null && sleep 2
    adb -s "$SERIAL" reverse --remove tcp:9000 2>/dev/null
    pkill -P $$ 2>/dev/null
    [[ -n ${NSPID:-} ]] && kill "$NSPID" 2>/dev/null
    echo; echo "artifacts in $S"
}
trap cleanup EXIT

# --- namespace + bridges (see emulator-userns.md, "Real USB device")
unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
NSPID=$!; sleep 0.5
NS="nsenter -t $NSPID -U -n --preserve-credentials"

if [[ $PFX == 32 ]]; then ADDRS="--address $OFFICE.2/32 --address $LAB.2/32"
else ADDRS="--address $OFFICE.2/24 --address $LAB.2/24"; fi
# shellcheck disable=SC2086
$NS "$BIN" run --no-adb --session "$SESSION" --secret-file "$SECRET" --tun phone0 $ADDRS \
    --route $OFFICE.0/24 --route $LAB.0/24 --route 0.0.0.0/0 --dns $OFFICE.1 > "$S/tunnel.log" 2>&1 &
TPID=$!
for _ in $(seq 1 50); do grep -q "waiting for a client" "$S/tunnel.log" 2>/dev/null && break; sleep 0.1; done
HP=$(sed 's/\x1b\[[0-9;]*m//g' "$S/tunnel.log" | grep -oE 'host_port=[0-9]+' | grep -oE '[0-9]+$' | head -1)
[[ -n $HP ]] || { cat "$S/tunnel.log"; exit 2; }
$NS socat UNIX-LISTEN:"$S/tun.sock",fork,unlink-early TCP4:127.0.0.1:"$HP" &
sleep 0.3; socat TCP4-LISTEN:"$HP",bind=127.0.0.1,fork,reuseaddr UNIX:"$S/tun.sock" &
sleep 0.3
adb -s "$SERIAL" reverse --remove-all >/dev/null 2>&1 || true
adb -s "$SERIAL" reverse tcp:9000 tcp:"$HP" >/dev/null
"$BIN" bootstrap-record --session "$SESSION" --secret-file "$SECRET" | adb -s "$SERIAL" shell content write --uri content://dev.routedroid.phase0.bootstrap/record
adb -s "$SERIAL" shell am start -n dev.routedroid.phase0/.BootstrapActivity --es session "$SESSION" --ei device_port 9000 >/dev/null
for _ in $(seq 1 300); do grep -q "session Active" "$S/tunnel.log" && break; sleep 0.1; done
grep -q "session Active" "$S/tunnel.log" || { echo "session did not become Active (accept the VPN prompt?)"; tail -3 "$S/tunnel.log"; exit 3; }
log "session Active with /$PFX aliases"

$NS ip addr add $OFFICE.1/24 dev phone0
$NS ip addr add $LAB.1/24 dev phone0
$NS ip addr add $INET.1/24 dev phone0

# --- observed Android state
log "Android tun0 addresses and routes (observable state)"
adb -s "$SERIAL" shell "ip -4 addr show tun0 | grep inet; ip rule | grep tun0; for t in \$(ip rule | grep -oE 'lookup [0-9]+' | awk '{print \$2}' | sort -u); do ip route show table \$t 2>/dev/null | grep tun0 | sed \"s/^/table \$t: /\"; done" | tee "$S/android-routes.txt"
adb -s "$SERIAL" shell "dumpsys connectivity | grep -A3 -iE 'VPN.*tun0|InterfaceName: tun0' | grep -iE 'LinkAddresses|Routes' | head -4" | tee "$S/android-lp.txt" || true

# --- source selection: sniff phone0 in the ns while the phone pings each LAN
cat > "$S/sniff.py" <<'EOF'
import socket, struct, sys, time
s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW, socket.htons(0x0800)); s.bind((sys.argv[1], 0)); s.settimeout(0.5)
end = time.time() + float(sys.argv[2])
while time.time() < end:
    try: pkt, meta = s.recvfrom(65535)
    except socket.timeout: continue
    if meta[2] == socket.PACKET_OUTGOING or pkt[9] != 1: continue
    ihl = (pkt[0] & 15) * 4
    if pkt[ihl] != 8: continue   # echo request only
    print(socket.inet_ntoa(pkt[12:16]), "->", socket.inet_ntoa(pkt[16:20]), flush=True)
EOF
$NS python3 "$S/sniff.py" phone0 14 > "$S/sniff.txt" 2>&1 &
sleep 1
for dst in $OFFICE.1 $LAB.1 $INET.1; do
    adb -s "$SERIAL" shell "ping -c 2 -W 1 $dst" | grep -E "transmitted" | sed "s/^/phone ping $dst: /"
done
sleep 4
log "sources chosen by Android (unbound ICMP sockets)"
sort "$S/sniff.txt" | uniq -c | tee "$S/sources.txt"
src_for() { awk -v d="$1" '$3==d{print $1}' "$S/sniff.txt" | sort -u | tr '\n' ' '; }
check "office LAN ($OFFICE.1) uses source $OFFICE.2"     test "$(src_for $OFFICE.1)" = "$OFFICE.2 "
check "lab LAN ($LAB.1) uses source $LAB.2"               test "$(src_for $LAB.1)" = "$LAB.2 "
check "default route ($INET.1) uses primary $OFFICE.2"    test "$(src_for $INET.1)" = "$OFFICE.2 "
check "office LAN reply received"  bash -c "adb -s $SERIAL shell 'ping -c 1 -W 1 $OFFICE.1' | grep -q ' 0% packet loss'"
check "lab LAN reply received"     bash -c "adb -s $SERIAL shell 'ping -c 1 -W 1 $LAB.1' | grep -q ' 0% packet loss'"

# --- TCP source selection (connected socket, unbound)
log "TCP connect source per destination (host listeners on :7001)"
for net in $OFFICE $LAB $INET; do
    $NS timeout 6 python3 -c "
import socket; s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(('$net.1',7001)); s.listen(1); s.settimeout(5)
c,a=s.accept(); print('$net.1 <- peer', a[0])" > "$S/tcp-$net.txt" 2>&1 &
    LPID=$!; sleep 0.5; adb -s "$SERIAL" shell "timeout 3 nc $net.1 7001 </dev/null" >/dev/null 2>&1 || true; wait $LPID || true; cat "$S/tcp-$net.txt"
done
check "TCP to office from $OFFICE.2"  grep -q "peer $OFFICE.2" "$S/tcp-$OFFICE.txt"
check "TCP to lab from $LAB.2"        grep -q "peer $LAB.2" "$S/tcp-$LAB.txt"
check "TCP default from $OFFICE.2"    grep -q "peer $OFFICE.2" "$S/tcp-$INET.txt"

# --- inbound to both aliases
log "inbound to each alias"
check "host -> $OFFICE.2 ICMP" bash -c "$NS ping -c 2 -W 1 $OFFICE.2 | grep -q ' 0% packet loss'"
check "host -> $LAB.2 ICMP"    bash -c "$NS ping -c 2 -W 1 $LAB.2 | grep -q ' 0% packet loss'"
adb -s "$SERIAL" shell "nc -l -p 7002 -s 0.0.0.0 > /data/local/tmp/ma1.txt 2>&1 &"; sleep 0.5
$NS bash -c "echo to-office | timeout 3 nc -N $OFFICE.2 7002" || true; sleep 0.5
check "TCP inbound to $OFFICE.2 (wildcard listener)" bash -c "adb -s $SERIAL shell cat /data/local/tmp/ma1.txt | grep -q to-office"
adb -s "$SERIAL" shell "nc -l -p 7003 -s 0.0.0.0 > /data/local/tmp/ma2.txt 2>&1 &"; sleep 0.5
$NS bash -c "echo to-lab | timeout 3 nc -N $LAB.2 7003" || true; sleep 0.5
check "TCP inbound to $LAB.2 (wildcard listener)" bash -c "adb -s $SERIAL shell cat /data/local/tmp/ma2.txt | grep -q to-lab"
adb -s "$SERIAL" shell "nc -l -p 7004 -s $LAB.2 > /data/local/tmp/ma3.txt 2>&1 &"; sleep 0.5
$NS bash -c "echo to-lab-bound | timeout 3 nc -N $LAB.2 7004" || true; sleep 0.5
check "TCP inbound to $LAB.2 (listener bound to $LAB.2)" bash -c "adb -s $SERIAL shell cat /data/local/tmp/ma3.txt | grep -q to-lab-bound"

echo; echo "RESULT /$PFX: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
