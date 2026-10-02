# Routedroid Android

The phone end of the version-1 wire protocol (`protocol/version-1.md`): a `VpnService` that
carries raw IPv4 between the phone and the PC over `adb reverse`.

| Module | Package | Role |
|---|---|---|
| `:protocol` | `dev.routedroid.protocol` | Plain JVM library: framing, bodies (strict JSON codec), state allowlist, mutual HMAC, bootstrap record, IPv4 checks. Tested against `protocol/fixtures/`. |
| `:app` | `dev.routedroid` | The product app. minSdk 26, targetSdk/compileSdk 36. |
| `:hostile` | `dev.routedroid.hostile` | Test-only third-party app that attacks the app's exported surface (`testing/hostile`). Included only with `-Proutedroid.hostile`; never part of a release. |

The app's version comes from `[workspace.package]` in `host/Cargo.toml`, the single product
version. HELLO's `app` field reports it to the host.

## Toolchain

| Tool | Version | Notes |
|---|---|---|
| Gradle | 9.2.1 | through `./gradlew` |
| Android Gradle Plugin | 9.0.1 | built-in Kotlin (2.2.10); no separate `kotlin-android` plugin |
| JDK for the Gradle daemon | 17 | pinned by `gradle/gradle-daemon-jvm.properties` |
| Android SDK | platform 36 | `local.properties`: `sdk.dir=/home/<you>/Android/Sdk` |

Every version lives in `gradle/libs.versions.toml`. Runtime dependencies are AndroidX core,
appcompat, activity, lifecycle-runtime, Material and kotlinx-coroutines. Tests use JUnit 4.

Gradle needs a JDK 17 it can detect, for example a system package, SDKMAN, `~/.gradle/jdks/`
or `org.gradle.java.installations.paths` in `~/.gradle/gradle.properties`. The `gradlew`
launcher itself runs on any JDK.

## Build and test

```
cd android
./gradlew :protocol:test :app:testDebugUnitTest :app:lintDebug :app:assembleDebug
```

- `:protocol:test` replays the golden fixtures: frames, auth vectors, bootstrap records,
  bodies, state allowlists and IPv4.
- `:app:testDebugUnitTest` runs `link/` and `transport/` over real loopback sockets against
  a fake host and an in-memory TUN.
- Lint is strict: warnings are errors.
- The APK lands in `app/build/outputs/apk/debug/app-debug.apk`.

### Release

`./gradlew :app:assembleRelease` builds with R8 and resource shrinking. Without a key it
produces `app-release-unsigned.apk` to sign elsewhere. To sign it in the build, give the
key through Gradle properties (for example in `~/.gradle/gradle.properties`) or the
environment; nothing about the key lives in the repository:

| Property | Environment |
|---|---|
| `routedroid.signing.storeFile` | `ROUTEDROID_SIGNING_STORE_FILE` |
| `routedroid.signing.storePassword` | `ROUTEDROID_SIGNING_STORE_PASSWORD` |
| `routedroid.signing.keyAlias` | `ROUTEDROID_SIGNING_KEY_ALIAS` |
| `routedroid.signing.keyPassword` | `ROUTEDROID_SIGNING_KEY_PASSWORD` (defaults to the store password) |

## How a session starts

The host (`routedroid start`) does all of this; nothing on the phone is started by hand.

1. `adb reverse tcp:DEVICE_PORT tcp:HOST_PORT` to a loopback listener on the PC.
2. Stream the 80-byte bootstrap record (§7.1) with
   `adb shell content write --uri content://dev.routedroid.bootstrap/record`.
   The record carries the port, the session id and the secret.
3. `adb shell am start -n dev.routedroid/.bootstrap.BootstrapActivity --es session <id>` (§7.2).

`BootstrapProvider` accepts the record only from the shell UID and holds it for 60 s. A launch
whose session does not match the pending record does nothing and leaves the record in place.

A launch that matches hands the record to `DeviceLink`, the process-wide owner of at most one
session. A newer launch supersedes the current session. Each `Session` runs on its own thread
through these phases:

1. **Authenticate** (`HostAuthenticator`): connect to `127.0.0.1:<record port>`, then
   HELLO / HELLO_ACK / AUTH. A bad `host_proof` closes the socket silently.
2. **Consent** (`BootstrapActivity`): notification permission (API 33+), then VPN consent.
   It must arrive within the 120 s configure deadline.
3. **Configure**:
   - `RoutedroidVpnService` attaches as the `VpnHost`.
   - `CONFIGURE_VPN` is checked against the negotiated MTU.
   - The VPN is established and `VPN_READY` sent.
4. **Active** (`PacketPath`): five threads (TUN reader and writer, socket reader and writer,
   keepalive). They run until something ends the session:
   - STOP or ERROR from the host;
   - Stop, in the app or its notification;
   - `onRevoke()`;
   - a protocol violation;
   - 30 s without a frame from the host. The app sends a PING after every 10 s of silence.

Every end is a `SessionEnd`. If the host was authenticated, the socket's last frame says why
the session ended:

| End | Last frame |
|---|---|
| User stop | STOP |
| Local failure | `VPN_ERROR` with a §4.6 code |
| Host-caused end | none |

Useful while testing:

```
adb -s SERIAL logcat -s DeviceLink PacketPath BootstrapProvider   # "session ended: <end>"
adb -s SERIAL shell am start -n dev.routedroid/.ui.MainActivity   # status, diagnostics, Stop
```

## Source map (`app/src/main/java/dev/routedroid`)

| Path | Role |
|---|---|
| `RoutedroidApp.kt` | Builds the one `DeviceLink`. |
| `bootstrap/BootstrapProvider.kt`, `RecordVault.kt` | The shell-only record sink, and the single 60 s record slot. |
| `bootstrap/HostAuthenticator.kt` | §5 steps 1–3 on the app side; wipes the secret on every path. |
| `bootstrap/BootstrapActivity.kt` | §7.2 entry point. It finishes before showing anything unless the launch matched; otherwise it asks for consent. |
| `link/DeviceLink.kt`, `Session.kt`, `Configure.kt` | Session ownership, the phase sequence and the configure deadline. |
| `link/SessionEnd.kt`, `LinkState.kt`, `VpnHost.kt` | Why a session ended (and its last frame), the published state, and the service seam. |
| `transport/` | `Connection` (socket and frame reader), `PacketPath` and `Pumps`, bounded `Slot` pools, `Keepalive` and `Traffic` counters. |
| `vpn/RoutedroidVpnService.kt`, `VpnBuilderConfig.kt`, `TunDevice.kt`, `LinkNotification.kt` | Foreground service, `VpnService.Builder`, the poll-and-wake TUN wrapper, and the notification with Stop. |
| `ui/MainActivity.kt`, `StatusText.kt` | Status, diagnostics and Stop. |

Packet path properties:

- **TUN I/O:** `Os.read`/`Os.write` on the TUN fd, which is polled together with a wake
  pipe, so a stop never closes an fd under a blocked read. `EINTR` is retried; a short
  write is fatal.
- **Allocation:** none per packet. Each direction has a fixed slot pool, about 1 MiB in
  total. A full host-to-phone queue blocks the reader, which is backpressure. A full queue
  in the other direction drops the packet and counts it.
- **IPv4 checks:** a host packet that fails the §6 IPv4 checks is dropped and counted, while
  a bad *frame* is a violation. Non-IPv4 packets read from the TUN are dropped and counted.
  With no IPv6 address or route configured, the platform blocks IPv6.

## Not in version 1

- `LEASE_UPDATE` or reconfiguration: a session gets one `CONFIGURE_VPN` and one VPN;
- reconnect and always-on (`SUPPORTS_ALWAYS_ON=false`);
- IPv6.

The foreground service type is `specialUse`, which is fine for sideloading; Play would need
a justification.

## Device verification

The rigs in `integration-tests/emulator/` are described in its README, along with the latest
results.
