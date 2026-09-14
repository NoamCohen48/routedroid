#!/usr/bin/env bash
# Phase 0 §3.1 on a REAL LAN: give the phone an address from the PC's own
# subnet, route it through phoneN, answer ARP for it with proxy ARP, and
# forward with a deny-first nftables table. Manual address (no DHCP yet).
#
#   sudo ./lan-proxyarp.sh SERIAL LAN_IF PHONE_IP [DNS]
#   e.g. sudo ./lan-proxyarp.sh 85e49002 eno1 10.100.102.222 10.100.102.1
#
# Runs until Ctrl-C, then removes everything it added and prints a baseline
# diff. Only run this on a network you own; the address must be unused.
set -euo pipefail

SERIAL=${1:?serial}; LAN_IF=${2:?lan interface}; PHONE_IP=${3:?phone ip}; DNS=${4:-}
TUN=${TUN:-phone0}
MTU=${MTU:-1400}
HERE=$(cd "$(dirname "$0")" && pwd)
BIN=${BIN:-$HERE/../../host/target/release/phase0-tunnel}
ADB_USER=${SUDO_USER:-$USER}          # adb server belongs to the desktop user
LOG=${LOG:-/tmp/routedroid-lan-tunnel.log}

[[ $EUID -eq 0 ]] || { echo "run with sudo"; exit 2; }
[[ -x $BIN ]] || { echo "missing $BIN (cargo build --release in host/)"; exit 2; }
for t in ip nft sysctl; do command -v $t >/dev/null || { echo "missing $t"; exit 2; }; done

HOST_IP=$(ip -4 -o addr show dev "$LAN_IF" | awk '{print $4}' | cut -d/ -f1 | head -1)
LAN_NET=$(ip -4 -o addr show dev "$LAN_IF" | awk '{print $4}' | head -1)
[[ -n $HOST_IP ]] || { echo "$LAN_IF has no IPv4 address"; exit 2; }
[[ -z ${DNS} ]] && DNS=$(ip -4 route show default dev "$LAN_IF" | awk '{print $3}' | head -1)
echo "host=$HOST_IP lan=$LAN_NET phone=$PHONE_IP dns=$DNS tun=$TUN"

# ---------------------------------------------------------------- baseline
TMP=$(mktemp -d)
snapshot() {
    ip -4 route show > "$1/route"
    nft list ruleset > "$1/nft" 2>/dev/null || true
    for k in net.ipv4.ip_forward net.ipv4.conf.all.forwarding net.ipv4.conf.all.proxy_arp \
             net.ipv4.conf.$LAN_IF.forwarding net.ipv4.conf.$LAN_IF.proxy_arp \
             net.ipv4.conf.$LAN_IF.rp_filter net.ipv4.conf.all.rp_filter; do
        printf '%s=%s\n' "$k" "$(sysctl -n "$k")"
    done > "$1/sysctl"
    ip -br link | awk '{print $1}' | sort > "$1/links"
}
mkdir -p "$TMP/before" "$TMP/after"; snapshot "$TMP/before"
echo "baseline: $(tr '\n' ' ' < "$TMP/before/sysctl")"
PA0=$(sysctl -n net.ipv4.conf.$LAN_IF.proxy_arp); FW0=$(sysctl -n net.ipv4.conf.$LAN_IF.forwarding)

# neighbour check: refuse an address that already answers ARP
if ip neigh show "$PHONE_IP" dev "$LAN_IF" 2>/dev/null | grep -qE 'lladdr'; then
    echo "$PHONE_IP already has an ARP owner on $LAN_IF; refusing"; exit 3
fi

TUNNEL_PID=""
cleanup() {
    trap - EXIT INT TERM   # run once: INT would otherwise be followed by EXIT
    set +e
    echo; echo "--- cleanup"
    [[ -n $TUNNEL_PID ]] && kill -INT "$TUNNEL_PID" 2>/dev/null && for _ in $(seq 1 50); do kill -0 "$TUNNEL_PID" 2>/dev/null || break; sleep 0.1; done
    nft delete table inet routedroid_p0 2>/dev/null
    ip route del "$PHONE_IP/32" dev "$TUN" 2>/dev/null
    sysctl -q -w net.ipv4.conf.$LAN_IF.proxy_arp=$PA0
    sysctl -q -w net.ipv4.conf.$LAN_IF.forwarding=$FW0
    sudo -u "$ADB_USER" adb -s "$SERIAL" reverse --list 2>/dev/null | grep -q 'tcp:9000' && \
        sudo -u "$ADB_USER" adb -s "$SERIAL" reverse --remove tcp:9000 2>/dev/null
    sleep 0.5; snapshot "$TMP/after"
    if diff -r "$TMP/before" "$TMP/after" >/dev/null; then echo "BASELINE RESTORED"; else echo "BASELINE DIFFERS:"; diff -r "$TMP/before" "$TMP/after"; fi
    rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

# ------------------------------------------------- deny-first nftables (§11)
cat > "$TMP/rules.nft" <<EOF
table inet routedroid_p0 {
    counter spoof_drop {}
    counter input_drop {}
    counter forward_drop {}
    counter postrouting_drop {}
    counter output_drop {}
    counter phone_to_lan {}
    counter lan_to_phone {}
    counter phone_to_host {}
    counter host_to_phone {}
    chain raw_prerouting {
        type filter hook prerouting priority -300; policy accept;
        iifname "$TUN" ip saddr != $PHONE_IP counter name "spoof_drop" drop
    }
    chain input {
        type filter hook input priority -10; policy accept;
        iifname "$TUN" ip saddr $PHONE_IP ip daddr $HOST_IP counter name "phone_to_host" accept
        iifname "$TUN" counter name "input_drop" drop
    }
    chain forward {
        type filter hook forward priority -10; policy accept;
        # primary interface: Internet allowed (0.0.0.0/0), replies from anywhere
        iifname "$TUN" oifname "$LAN_IF" ip saddr $PHONE_IP counter name "phone_to_lan" accept
        iifname "$LAN_IF" oifname "$TUN" ip daddr $PHONE_IP counter name "lan_to_phone" accept
        iifname "$TUN" counter name "forward_drop" drop
        oifname "$TUN" counter name "forward_drop" drop
    }
    chain postrouting {
        type filter hook postrouting priority -10; policy accept;
        oifname "$TUN" ip daddr != $PHONE_IP counter name "postrouting_drop" drop
    }
    chain output {
        type filter hook output priority -10; policy accept;
        oifname "$TUN" ip daddr $PHONE_IP counter name "host_to_phone" accept
        oifname "$TUN" meta nfproto ipv4 counter name "output_drop" drop
    }
}
EOF
nft -f "$TMP/rules.nft"
echo "nftables table inet routedroid_p0 installed (deny-first)"

# ------------------------------------------------------------------ tunnel
# adb must talk to the desktop user's adb server; run the tunnel as root but
# with the user's adb via PATH is fine (client connects to 127.0.0.1:5037).
"$BIN" run --serial "$SERIAL" --tun "$TUN" --address "$PHONE_IP/32" --mtu "$MTU" \
    --route 0.0.0.0/0 --dns "$DNS" > "$LOG" 2>&1 &
TUNNEL_PID=$!
for _ in $(seq 1 100); do ip link show "$TUN" >/dev/null 2>&1 && break; sleep 0.1; done
ip link show "$TUN" >/dev/null 2>&1 || { echo "tunnel did not create $TUN; see $LOG"; exit 4; }

# ---------------------------------------- route, forwarding, proxy ARP (§9-12)
ip route add "$PHONE_IP/32" dev "$TUN" src "$HOST_IP"
sysctl -q -w net.ipv4.conf.$TUN.forwarding=1
sysctl -q -w net.ipv4.conf.$LAN_IF.forwarding=1
sysctl -q -w net.ipv4.conf.$LAN_IF.proxy_arp=1
echo "route $PHONE_IP/32 dev $TUN src $HOST_IP; proxy_arp on $LAN_IF; per-interface forwarding on"
echo "global ip_forward=$(sysctl -n net.ipv4.ip_forward) (unchanged)"
echo
echo "tunnel running (pid $TUNNEL_PID, log $LOG). Accept the VPN prompt on the phone."
echo "Ctrl-C to stop and restore."
wait "$TUNNEL_PID" || true
TUNNEL_PID=""
