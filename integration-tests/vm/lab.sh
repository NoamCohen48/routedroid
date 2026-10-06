#!/usr/bin/env bash
# A LAN of KVM guests, run unprivileged, where Routedroid gets real root and
# real systemd: `router` (DHCP, DNS, NAT, and a LAN peer) and `host` (the PC).
#
#   ./lab.sh up [router|host...]      create on first use, boot, wait for cloud-init
#   PHONE=04e8:6860 ./lab.sh up host  pass that USB device (the phone) to host
#   ./lab.sh ssh NAME [command...]    as user dev (passwordless sudo)
#   ./lab.sh push                     build, copy and install Routedroid on host
#   ./lab.sh down [NAME...] | status | destroy NAME
#
# Each guest has mgmt0 (qemu user networking: ssh from here on 127.0.0.1:220N,
# and the router's uplink) and lan0 on a multicast segment shared by every
# guest: 192.168.80.0/24, the router at .1, leases .100-.150.
# State lives in ~/.cache/routedroid-vm (override with LAB_DIR).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
LAB=${LAB_DIR:-$HOME/.cache/routedroid-vm}
BASE=$LAB/debian-13-generic-amd64.qcow2
KEY=$LAB/id_lab
declare -A INDEX=([router]=1 [host]=2) MEM=([router]=384 [host]=1024)
SSH_OPTS=(-i "$KEY" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=3)

die() { echo "lab: $*" >&2; exit 1; }
known() { [[ -n ${INDEX[$1]:-} ]] || die "no guest named $1 (router, host)"; }
port() { echo $((2200 + INDEX[$1])); }
pidof_vm() { cat "$LAB/$1/pid" 2>/dev/null; }
running() { local pid; pid=$(pidof_vm "$1") && kill -0 "$pid" 2>/dev/null; }
vssh() { local name=$1; shift; ssh "${SSH_OPTS[@]}" -p "$(port "$name")" dev@127.0.0.1 "$@"; }

seed() { # seed NAME: the cloud-init NoCloud disk
    local name=$1 dir=$LAB/$1 n=${INDEX[$1]} lan
    if [[ $name == router ]]; then
        lan='    addresses: [192.168.80.1/24]'
    else
        lan=$'    dhcp4: true\n    dhcp4-overrides: {route-metric: 100}'
    fi
    sed "s|@KEY@|$(cat "$KEY.pub")|" "$HERE/$name.yaml" > "$dir/user-data"
    printf 'instance-id: %s-1\nlocal-hostname: %s\n' "$name" "$name" > "$dir/meta-data"
    MGMT_METRIC=$([[ $name == router ]] && echo 100 || echo 1000)
    python3 - "$HERE/network.yaml" "$dir/network-config" "$n" "$MGMT_METRIC" "$lan" <<'EOF'
import sys
src, dst, n, metric, lan = sys.argv[1:]
text = open(src).read().replace("@MGMT_MAC@", f"52:54:00:00:00:0{n}") \
    .replace("@LAN_MAC@", f"52:54:00:80:00:0{n}").replace("@MGMT_METRIC@", metric) \
    .replace("@LAN@", lan)
open(dst, "w").write(text)
EOF
    rm -f "$dir/seed.img"
    mkfs.vfat -n CIDATA -C "$dir/seed.img" 1024 >/dev/null
    mcopy -i "$dir/seed.img" "$dir/user-data" "$dir/meta-data" "$dir/network-config" ::
}

create() {
    local dir=$LAB/$1
    [[ -f $BASE ]] || die "missing $BASE (see README.md)"
    [[ -f $KEY ]] || ssh-keygen -q -t ed25519 -N '' -C routedroid-lab -f "$KEY"
    mkdir -p "$dir"
    qemu-img create -q -f qcow2 -b "$BASE" -F qcow2 "$dir/disk.qcow2" 8G
    seed "$1"
}

boot() {
    local name=$1 dir=$LAB/$1 n=${INDEX[$1]} usb=()
    if [[ $name == host && -n ${PHONE:-} ]]; then
        usb=(-device "qemu-xhci,id=xhci" -device "usb-host,bus=xhci.0,vendorid=0x${PHONE%:*},productid=0x${PHONE#*:}")
    fi
    qemu-system-x86_64 -name "$name" -enable-kvm -cpu host -smp 2 -m "${MEM[$name]}" \
        -drive "file=$dir/disk.qcow2,if=virtio" -drive "file=$dir/seed.img,if=virtio,format=raw" \
        -netdev "user,id=mgmt,hostfwd=tcp:127.0.0.1:$(port "$name")-:22" \
        -device "virtio-net-pci,netdev=mgmt,mac=52:54:00:00:00:0$n" \
        -netdev socket,id=lan,mcast=230.0.0.1:5580,localaddr=127.0.0.1 \
        -device "virtio-net-pci,netdev=lan,mac=52:54:00:80:00:0$n" \
        "${usb[@]}" -display none -serial "file:$dir/console.log" \
        -daemonize -pidfile "$dir/pid"
}

wait_ready() {
    local _
    for _ in $(seq 1 120); do vssh "$1" true 2>/dev/null && break; sleep 2; done
    vssh "$1" 'cloud-init status --wait >/dev/null; cloud-init status' || die "$1: cloud-init failed"
}

up() {
    local name
    for name in "$@"; do
        known "$name"
        if running "$name"; then echo "$name: already running"; continue; fi
        [[ -f $LAB/$name/disk.qcow2 ]] || create "$name"
        boot "$name"
    done
    for name in "$@"; do wait_ready "$name"; echo "$name: up (ssh port $(port "$name"))"; done
}

down() {
    local name pid _
    for name in "$@"; do
        running "$name" || continue
        vssh "$name" sudo poweroff 2>/dev/null || true
        pid=$(pidof_vm "$name")
        for _ in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
        kill "$pid" 2>/dev/null || true
        echo "$name: down"
    done
}

cmd=${1:-status}; shift || true
case $cmd in
    up) if (($#)); then up "$@"; else up router host; fi ;;
    down) if (($#)); then down "$@"; else down host router; fi ;;
    ssh) known "$1"; vssh "$@" ;;
    push) "$HERE/push.sh" ;;
    status) for name in router host; do printf '%-7s %s\n' "$name" "$(running "$name" && echo running || echo stopped)"; done ;;
    destroy) known "$1"; down "$1"; rm -rf "${LAB:?}/$1"; echo "$1: destroyed" ;;
    *) die "unknown command $cmd" ;;
esac
