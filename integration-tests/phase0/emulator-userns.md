# Running the Phase 0 tunnel against an Android emulator without root

`phase0-tunnel run` needs `CAP_NET_ADMIN` for the TUN. Without sudo, an
unprivileged user+network namespace provides it; the emulator's adbd port is
bridged into that namespace over a Unix socket (Unix sockets cross network
namespaces, TCP loopback does not).

```bash
S=/tmp/rd-emu; mkdir -p $S
# 1. outer bridge: unix socket -> emulator adbd (host namespace)
socat UNIX-LISTEN:$S/adbd.sock,fork,unlink-early TCP:127.0.0.1:5555 &
# 2. namespace holder
unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
NS="nsenter -t $! -U -n --preserve-credentials"
# 3. inner bridge + private adb server inside the namespace
$NS socat TCP-LISTEN:5555,bind=127.0.0.1,fork,reuseaddr UNIX:$S/adbd.sock &
$NS adb start-server && $NS adb connect 127.0.0.1:5555
# 4. tunnel (adb reverse and am start happen through the inner adb server)
$NS host/target/release/phase0-tunnel run --serial 127.0.0.1:5555 \
    --tun phone0 --address 10.77.0.2/32 --dns 10.77.0.1 &
# accept the notification + VPN consent dialogs on the emulator, then:
$NS ip addr add 10.77.0.1/24 dev phone0
$NS ping -c 3 10.77.0.2
adb shell ping -c 3 10.77.0.1
```

`adb reverse` works because the inner adb server owns the transport: the
device-side connection to `127.0.0.1:9000` is forwarded by that server to the
tunnel's loopback listener in the same namespace.

## Results, 2026-09-14, Android 14 (API 34, google_apis_playstore x86_64)

| §3.1 acceptance item | Result |
|---|---|
| Android pings the PC | PASS (rtt ~2–44 ms) |
| PC pings the Android VPN address | PASS |
| LAN host initiates TCP to Android | PASS: 200 000 B into toybox `nc -l` on the phone, md5 equal |
| LAN host initiates UDP to Android | PASS |
| Android initiates TCP/UDP to host | PASS: 100 000 B out, md5 equal; UDP received from `10.77.0.2` |
| Original address forwarded, no NAT | PASS: capture on `phone0` shows `10.77.0.2 -> 10.77.0.1` unchanged |
| ADB loss does not grow memory | PASS: host RSS 5172 kB before/during/after a 6 s ADB stall under `ping -f -s 1200` |
| One raw IPv4 packet per TUN read/write | PASS (host validates `total_length == body`) |
| Teardown restores baseline | PASS: SIGINT -> STOP -> Android `tun0` gone, service ended cleanly; host `phone0` gone, reverse mapping removed |
| ADB stable while VPN owns default route | PASS on emulator only (adbd is not routed through the VPN there; real USB/Wi-Fi ADB is still an open gate) |

"LAN host" above is the host namespace itself; proxy ARP/LAN forwarding was
proven separately by `netns-tunnel.sh`.

## Bug found and fixed

The first run wedged after ~10 packets: the Android app used
`SocketChannel.socket().getInputStream()/getOutputStream()`. Both the
`SocketAdaptor` input stream and `Channels.newOutputStream` synchronize on
`SocketChannel.blockingLock()` for the entire blocking call, so a reader
blocked in `read()` starves the writer until the peer sends something. Fixed
by reading/writing the `SocketChannel` directly (independent read/write locks)
with a watchdog for the handshake timeout instead of `soTimeout`.
