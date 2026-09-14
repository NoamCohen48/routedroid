# Routedroid Android — Phase 0 probe

Throwaway-quality Android side of implementation-plan §3.1 ("Minimal Packet Tunnel").
It implements `protocol/phase0-draft.md` exactly and nothing more. It will be replaced,
not evolved, once the Phase 0 gates are recorded.

Package / applicationId: `dev.routedroid.phase0`. minSdk 26, targetSdk/compileSdk 36.

## Toolchain

| Tool | Version | Notes |
|---|---|---|
| Gradle | 9.2.1 | via `./gradlew` (wrapper committed) |
| Android Gradle Plugin | 9.0.1 | built-in Kotlin support (bundles Kotlin Gradle Plugin 2.2.10); no separate `kotlin-android` plugin |
| Kotlin | 2.2.10 | provided by AGP's built-in Kotlin |
| JDK for the Gradle daemon | 17 | selected through `gradle/gradle-daemon-jvm.properties` (`toolchainVersion=17`) |
| Android SDK | platform 36, build-tools 36.0.0 | |

Dependencies are deliberately tiny: `androidx.core:core-ktx`, `androidx.appcompat:appcompat`,
`kotlinx-coroutines-android`, `org.json` (platform built-in), JUnit 4 for JVM tests.

### JDK selection

AGP 9 requires JDK 17+ and Gradle 9.2 does not support running its daemon on very new JDKs
(the machine this was scaffolded on only has JDK 26 system-wide). The project therefore pins
the **daemon** JVM to Java 17 with Gradle's daemon JVM criteria
(`gradle/gradle-daemon-jvm.properties`). The `gradlew` launcher itself may run on any JDK.

Gradle resolves the criteria against JDKs it can auto-detect, which includes JDKs it has
auto-provisioned before (`~/.gradle/jdks/`, e.g. `eclipse_adoptium-17-amd64-linux.2`). If none
is found and no toolchain download repository is configured, Gradle fails with a message
naming the missing JDK. Options:

- install a JDK 17 where Gradle can detect it (system package, SDKMAN, `JAVA_HOME`,
  `org.gradle.java.installations.paths=/path/to/jdk-17` in `~/.gradle/gradle.properties`), or
- as a local-only override, uncomment `org.gradle.java.home=/path/to/jdk-17` in
  `android/gradle.properties` (or put it in `~/.gradle/gradle.properties` so it is not
  committed).

### SDK location

`android/local.properties` (gitignored) must contain:

```
sdk.dir=/home/<you>/Android/Sdk
```

## Build and test

```
cd android
./gradlew :app:assembleDebug :app:testDebugUnitTest
```

Output APK: `app/build/outputs/apk/debug/app-debug.apk`.
Unit tests: `app/src/test/java/dev/routedroid/phase0/FrameCodecTest.kt` (header golden
vectors, limit enforcement, IPv4 validation, no-allocation on hostile lengths).

## Install and launch

```
adb -s SERIAL install -r app/build/outputs/apk/debug/app-debug.apk
```

The host side is expected to:

1. bind a loopback listener on `127.0.0.1:HOST_PORT`;
2. `adb -s SERIAL reverse tcp:DEVICE_PORT tcp:HOST_PORT`;
3. launch the bootstrap activity (line from the draft):

```
adb -s SERIAL shell am start -n dev.routedroid.phase0/.BootstrapActivity \
    --es session <id> --ei device_port <DEVICE_PORT>
```

`BootstrapActivity` requests `POST_NOTIFICATIONS` (API 33+) and VPN consent
(`VpnService.prepare`), then starts `Phase0VpnService` as a foreground service and opens
`MainActivity`. The first launch on a device shows the system VPN consent dialog, which must be
accepted by hand. If the extras are missing the activity shows an error and does nothing else.

`Phase0VpnService` then connects to `127.0.0.1:DEVICE_PORT`, `protect()`s the socket, sends
`HELLO`, waits for `HELLO_ACK` and `CONFIGURE_VPN`, establishes the VPN, answers `VPN_READY`
(or `VPN_ERROR`), and runs the two packet pumps until `STOP`, socket close, the Stop button in
`MainActivity`, or `onRevoke()`.

Useful while testing:

```
adb -s SERIAL logcat -s Phase0Vpn Phase0Bootstrap
adb -s SERIAL shell am start -n dev.routedroid.phase0/.MainActivity   # status UI
adb -s SERIAL shell am startservice -n dev.routedroid.phase0/.Phase0VpnService -a dev.routedroid.phase0.STOP
```

## Source map

| File | Role (architecture.md §4.1 name) |
|---|---|
| `FrameCodec.kt` | `FrameCodec`: 8-byte header encode/decode, per-type limits, IPv4 check. Pure Kotlin. |
| `Phase0VpnService.kt` | `RoutedroidVpnService` + `HostTransport` + `VpnConfigurator` + `PacketPump` collapsed into one class for the probe. |
| `BootstrapActivity.kt` | Exported, non-browsable entry point started by `am start`. |
| `MainActivity.kt` | Status text and Stop button. |
| `StatusStore.kt` | `StatusStore`: StateFlow for low-rate state, atomics for packet counters. |

Packet path details:

- One TCP stream. Frames are written whole through one `BufferedOutputStream` with an explicit
  `flush()` per frame; the buffered stream continues a partial TCP write until the frame is
  fully sent.
- VPN reads/writes use `android.system.Os.read/write` on the `ParcelFileDescriptor`, so a
  short VPN write is visible and treated as fatal (the session ends; the suffix is never
  re-submitted). `EINTR` (zero bytes transferred) is retried. The VPN read side uses `Os.poll`
  with a 500 ms timeout so teardown can stop the reader without relying on closing an fd out
  from under a blocked read.
- Each direction has a preallocated pool of 258 frame-sized slots (`mtu + 8` bytes) and a
  `Channel(256)` between reader and writer. The reader suspends when it cannot obtain a free
  slot, which is the backpressure required by architecture.md §8.4. No per-packet allocation.
- IPv4 validation (version nibble, IHL, total_length == body_length, total_length >= IHL*4)
  is applied to every packet received from the host; a violation closes the session. Packets
  read from the VPN that are not valid IPv4 (e.g. stray IPv6) are dropped and counted rather
  than sent.

## Deliberately missing vs architecture.md

This is the §3.1 probe only. Compared with the production design it has:

- **no authentication** (architecture §8.2): no session secret, no HMAC handshake. Anyone who
  can start the exported activity can trigger a VPN prompt. Added in Phase 0 §3.4.
- **no `BootstrapProvider`** (§8.1): no `adb shell content write` secret delivery, no shell-UID
  or `DUMP` permission checks.
- **no rate limiting** of bootstrap attempts.
- **no `LEASE_UPDATE` / reconfiguration**: one `CONFIGURE_VPN`, one VPN, no re-establish.
- **no persistence, no reconnect**, no notification Stop action, no diagnostics export
  (queue depth, RTT) beyond the on-screen counters.
- **foreground service type** is `specialUse` with a `PROPERTY_SPECIAL_USE_FGS_SUBTYPE`
  property, which is acceptable for a sideloaded probe; a Play-distributed app would need a
  declared justification.
- Only IPv4; no IPv6 addresses or routes.

## Acceptance items that need a real device

Everything in implementation-plan §3.1 "Acceptance" is end-to-end and cannot be checked by the
JVM unit tests here. Specifically, the following require a physical device (or emulator with a
working VPN stack) plus the host probe in an isolated network namespace or lab LAN:

- Android can ping the PC and one LAN host through the TUN path;
- the PC and a LAN host can ping the Android VPN address;
- a LAN host can initiate TCP and UDP traffic to test applications on Android;
- packet capture confirms Linux forwards the original phone address without NAT;
- disconnecting ADB does not cause unbounded memory growth (bounded pools here; verify with
  `dumpsys meminfo dev.routedroid.phase0` while the reverse mapping is removed);
- VPN reads and writes contain one raw IPv4 packet with no packet-information prefix;
- VPN consent flow, `onRevoke()` teardown, and foreground-service behaviour on API 26, 33/34+
  (notification permission, `specialUse` type) and a vendor-customised device;
- `protect()` actually keeps the loopback/adb socket outside the VPN once `0.0.0.0/0` is
  routed into it (this is the single most important thing to confirm on hardware).

What the unit tests do cover: exact header bytes, version/flags/type/limit enforcement,
rejection of `0xFFFFFFFF` lengths without allocation, and the IPv4 sanity rules.
