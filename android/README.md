# Routedroid Android

Android side of the version-1 wire protocol (`protocol/version-1.md`): a foreground
`VpnService` that becomes the phone end of the raw-IP tunnel the host opens over ADB.

Modules:

| Module | Package | Role |
|---|---|---|
| `:protocol` | `dev.routedroid.protocol` | Pure-Kotlin protocol library (framing, bodies, state allowlist, mutual HMAC, bootstrap record). Unit-tested on the JVM against `protocol/fixtures/`. |
| `:app` | `dev.routedroid` | The product app. minSdk 26, targetSdk/compileSdk 36. |
| `:hostile` | `dev.routedroid.hostile` | Throwaway third-party app that tries to write/read the bootstrap provider and launch the bootstrap activity; every attempt must be denied. |

The Phase 0 probe app (`dev.routedroid.phase0`) was replaced in place; it is in git history
before the "Phase 1 Android app" commit.

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
./gradlew :app:assembleDebug :protocol:testDebugUnitTest
```

Output APK: `app/build/outputs/apk/debug/app-debug.apk`. The protocol tests replay the
golden fixtures (frames, auth vectors, bootstrap record, state allowlists) and the IPv4 rules.

## How a session starts

The host (`routedroid start`) does all of this; nothing on the phone is started by hand.

1. bind a loopback listener on `127.0.0.1:HOST_PORT` and `adb reverse tcp:DEVICE_PORT tcp:HOST_PORT`;
2. stream the 80-byte bootstrap record (session id + secret, §7.1) to the provider's stdin:
   `adb shell content write --uri content://dev.routedroid.bootstrap/record`;
3. `adb shell am start -n dev.routedroid/.BootstrapActivity --es session <id> --ei device_port <n>`.

`BootstrapActivity` (rate limited: 3 launches per 10 s) takes the pending record — fail closed
if missing, expired (60 s), for another session, or lost to process death — connects to
`127.0.0.1:DEVICE_PORT` and runs HELLO / HELLO_ACK / AUTH (`HostHandshake`). A bad `host_proof`
closes the socket silently. Only after the host is authenticated does it ask for
`POST_NOTIFICATIONS` (API 33+) and VPN consent, start `RoutedroidVpnService` as a foreground
service and open `MainActivity`. If the user declines the consent the app sends
`VPN_ERROR vpn_permission_denied` so the host fails fast.

`RoutedroidVpnService` takes the authenticated socket from `PendingConnection`, `protect()`s
it, waits for `CONFIGURE_VPN` (15 s), validates it (`ConfigureVpn.decode` + MTU equality),
establishes the VPN, answers `VPN_READY` and runs the pumps until the host sends STOP or
ERROR, the socket closes, the Stop button, `onRevoke()`, a protocol violation
(`VPN_ERROR protocol_error`), or 30 s without any frame from the host (§5.1).

Useful while testing:

```
adb -s SERIAL logcat -s Bootstrap HostHandshake VpnService Session SocketReader BootstrapProvider
adb -s SERIAL shell am start -n dev.routedroid/.ui.MainActivity   # status UI + Stop
```

## Source map (`app/src/main/java/dev/routedroid`)

| File | Role |
|---|---|
| `BootstrapActivity.kt` | Exported, non-browsable §7.2 entry point; refuses without a record; consent flow. |
| `ConsentDenied.kt` | Reports a declined consent to the waiting host. |
| `bootstrap/BootstrapProvider.kt` | Exported, `DUMP`-guarded, shell-UID-checked `content write` sink (socketpair, not pipe — see decision record 0001 §3.4). |
| `bootstrap/BootstrapStore.kt`, `bootstrap/LaunchGate.kt` | Single-slot 60 s record holder; launch rate limit. |
| `bootstrap/HostHandshake.kt` | §5 steps 1–2 on the client side; wipes the secret on every path. |
| `session/PendingConnection.kt`, `session/StatusStore.kt` | Handoff of the authenticated socket + MTU to the service; UI status (StateFlow + atomic counters). |
| `transport/ChannelInput.kt`, `ChannelOutput.kt` | Direct `SocketChannel` I/O (the adaptor streams share one lock and deadlock; see Phase 0 notes). Header validated before any body allocation. |
| `transport/Slot.kt`, `Pumps.kt`, `TunReader.kt`, `TunWriter.kt`, `SocketReader.kt`, `Keepalive.kt` | Bounded slot pools (258 per direction, `Channel(256)`), the four pumps, PING/PONG and dead-peer detection. |
| `vpn/RoutedroidVpnService.kt`, `SessionRunner.kt`, `Configure.kt`, `VpnConfigurator.kt`, `VpnFailure.kt`, `VpnNotification.kt` | Service lifecycle; Negotiated → Active → Closed; `VpnService.Builder`; VPN_ERROR mapping; foreground notification. |
| `ui/MainActivity.kt`, `ui/StatusText.kt` | Status text and Stop button. |

Packet path properties (unchanged from the Phase 0 measurements):

- VPN reads/writes use `android.system.Os.read/write`; a short VPN write is fatal, `EINTR`
  is retried, the reader uses `Os.poll` (500 ms) so teardown never closes an fd under a
  blocked read.
- No per-packet allocation; backpressure by suspending on the free pool (architecture.md §8.4).
- Host packets failing the §6 IPv4 checks are dropped and counted; locally read non-IPv4
  packets (stray IPv6/ND) are dropped and counted; a bad *frame* is a violation and closes.

## Still missing vs architecture.md

- no `LEASE_UPDATE` / reconfiguration: one `CONFIGURE_VPN`, one VPN, no re-establish;
- no persistence, no reconnect, no notification Stop action, no diagnostics export;
- foreground service type `specialUse` (fine sideloaded; Play would need a justification);
- IPv4 only.

## Device verification

`integration-tests/phase1/emulator-userns.sh` (end to end, no root) and
`integration-tests/phase1/fake_host.py` (negative cases) — results in
`integration-tests/phase1/README.md`.
