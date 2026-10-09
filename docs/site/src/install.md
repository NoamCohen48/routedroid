# Install

## What you need

- **PC:**
  - Linux with nftables, policy routing and TUN (any current distribution);
  - systemd;
  - `adb` (Android platform-tools), with USB access to the phone (your distribution's
    `android-udev-rules`, or membership in `plugdev`).
- **Phone:**
  - Android 8.0 or newer;
  - USB debugging enabled, and this PC authorized;
  - the Routedroid app. A release's daemon installs it for you.
- **LAN:**
  - a wired or Wi-Fi network you may add a host to;
  - for DHCP, a server that leases one more address to the PC's MAC under another client
    identifier. Most home and office routers do.

## From the apt repository (Debian, Ubuntu)

Add the repository once. `apt upgrade` then brings new releases like any other package:

```sh
sudo mkdir -p /etc/apt/keyrings
curl -fsSL https://noamcohen48.github.io/routedroid/apt/routedroid.gpg \
    | sudo tee /etc/apt/keyrings/routedroid.gpg > /dev/null
echo "deb [signed-by=/etc/apt/keyrings/routedroid.gpg] https://noamcohen48.github.io/routedroid/apt stable main" \
    | sudo tee /etc/apt/sources.list.d/routedroid.list
sudo apt update && sudo apt install routedroid
sudo routedroid setup
```

The key signs only this repository; `signed-by` keeps apt from trusting it for any other.

## From packages


Download a package from the
[releases](https://github.com/NoamCohen48/routedroid/releases) or
[this site's downloads](https://noamcohen48.github.io/routedroid/download/), or build them with
`host/packaging/build.sh`, which writes them to `host/target/packages`.

```sh
sudo apt install ./routedroid_0.1.0_amd64.deb      # Debian, Ubuntu
sudo dnf install ./routedroid-0.1.0-1.x86_64.rpm   # Fedora
sudo routedroid setup
```

## From the tarball

On another systemd distribution (glibc 2.35 or newer, with nftables and adb installed), the
release's tarball holds the same programs and `install.sh`, which installs into `/usr/local`:

```sh
tar -xzf routedroid-0.1.0-linux-x86_64.tar.gz
sudo routedroid-0.1.0-linux-x86_64/install.sh
sudo /usr/local/bin/routedroid setup
```

## From source

You need Rust (the version is pinned in `host/rust-toolchain.toml`), and JDK 17 for the app.

```sh
cd host && cargo build --release && sudo ./install.sh && sudo /usr/local/bin/routedroid setup
(cd android && ./gradlew :app:assembleDebug)
adb install android/app/build/outputs/apk/debug/app-debug.apk
```

A daemon built from source carries no app, so install the app with `adb install` as above.
To build a daemon that carries one, set `ROUTEDROID_APK` to a signed APK's absolute path
(`host/packaging/build.sh` passes it through). Release builds of the app are described in
`android/README.md`.

## What gets installed

- `routedroid`, `routedroidd` and `routedroid-tui`, in `/usr/bin` (packages) or
  `/usr/local/bin`;
- the root helper, in `libexec/routedroid` under the same prefix;
- the helper's socket-activated units, enabled, and the daemon's user unit;
- group `routedroid`;
- a [policy](policy.md) that allows nothing yet.

- shell completions for bash, zsh and fish, and man pages (`man routedroid`,
  `man routedroid-start`, ...), in the usual places under `share/`. A new shell picks the
  completions up. Without the packages or `install.sh`, `routedroid completions SHELL`
  prints the script: for example
  `routedroid completions bash > ~/.local/share/bash-completion/completions/routedroid`.

## The app

A release's daemon carries the app. When a phone connects without it, or with an older
version, the daemon installs it first (the state reads `installing app`). The phone then
asks once for VPN permission. The release's APK is there too, for installing by hand.

Next: [Set up](setup.md).
