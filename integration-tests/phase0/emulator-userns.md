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

# Real USB device (host adb server owns the transport)

With a physical phone the host adb server must keep the USB transport, so
bridge the *tunnel listener* into the host namespace instead of adbd into the
inner one. `--no-adb` prints the port; do `reverse` and `am start` by hand:

```bash
$NS phase0-tunnel run --no-adb --session $SESSION --tun phone0 --address 10.77.0.2/32 &
HP=<port from the log>
$NS socat UNIX-LISTEN:$S/tun.sock,fork,unlink-early TCP4:127.0.0.1:$HP &   # TCP4: getaddrinfo fails in the bare netns
socat TCP4-LISTEN:$HP,bind=127.0.0.1,fork,reuseaddr UNIX:$S/tun.sock &
adb -s SERIAL reverse tcp:9000 tcp:$HP
adb -s SERIAL shell am start -n dev.routedroid.phase0/.BootstrapActivity --es session $SESSION --ei device_port 9000
```

Do not poke the listener with `nc` to "test the chain": the tunnel accepts one
connection and exits on a mid-frame close.

## Results, 2026-09-14, Samsung SM-J810G, Android 10 (API 29), USB ADB, no phone network

| Check | Result |
|---|---|
| VPN consent, foreground service, session Active | PASS |
| ICMP both directions | PASS (rtt 4–48 ms) |
| TCP host -> Android listener, 200 KB and 10 MB | PASS, md5 equal, 10 MB in 1.6 s |
| TCP Android -> host, 1 MB and 10 MB | PASS, md5 equal, 10 MB in 1.0 s |
| UDP Android -> host | PASS |
| UDP host -> Android | NOT TESTED: toybox `nc` on Android 10 cannot listen on UDP; needs a test app |
| ADB stall (6 s) under `ping -f -s 1200` | PASS: host RSS 6088 kB flat; phone app PSS 48 MB |
| ADB stable while VPN owns default route (USB) | PASS: ~25 000 packets, shell/logcat/install kept working |
| Teardown | PASS: `tun0` and `phone0` gone, "session ended cleanly" |

## Bug found and fixed

`InetAddress.getLoopbackAddress()` returns `::1` on this Android 10 build (it
returned `127.0.0.1` on the Android 14 emulator); adbd's `reverse` listener is
IPv4-only, so the app got `Connection refused`. The app now connects to an
explicit `127.0.0.1`.
