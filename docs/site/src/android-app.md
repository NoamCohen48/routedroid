# The Android app

The app (`dev.routedroid`) is the phone's end of the connection: an Android `VpnService`
that carries raw IPv4 between the phone and the PC over `adb reverse`. You never start it by
hand. `routedroid start` launches it for each connection.

## Installing it

- **With a release's daemon:** nothing to do. When a phone connects without the app, or
  with an older version, the daemon installs it first.
- **By hand:** `adb install` the APK from a release, or a build from `android/`.

If the phone refuses the daemon's app with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, another
build of the app, signed with another key, is installed. Uninstall it first:

```sh
adb uninstall dev.routedroid
```

## VPN permission

The first time, the phone asks whether Routedroid may set up a VPN. Unlock the phone and
allow it. The answer persists: later connections don't ask again, unless the app is
reinstalled. On Android 13 and newer, the app also asks to show notifications.

The phone has 2 minutes to answer. Without an answer the connection ends, so if a start
seems stuck at `handshaking`, look at the phone.

<!-- TODO: the locked-phone hint, once it lands. -->

## While connected

- The status bar shows the VPN key icon.
- A notification shows the connection, with Stop. Stop ends the connection from the phone.
- Opening the app shows its status and diagnostics, also with Stop.

Another VPN app on the phone takes over from Routedroid's, and that ends the connection.

## Trust

The phone accepts a session only from the PC that wrote its one-time secret over adb, and
both sides prove they know it. A phone holds one connection at a time. See the
[security model](security.md).

## For developers

`android/README.md` in the repository covers building, testing, release signing and how a
session starts, step by step.
