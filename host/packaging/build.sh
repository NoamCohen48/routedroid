#!/usr/bin/env bash
# Build the host side's .deb, .rpm and binary tarball (for any other systemd
# distribution: unpack, then sudo ./install.sh) into host/target/packages:
#
#   host/packaging/build.sh            (cargo build --release first)
#   NO_BUILD=1 host/packaging/build.sh (package host/target/release as it is)
#   ROUTEDROID_APK=app-release.apk host/packaging/build.sh
#                                      (routedroidd carries that signed app and
#                                      installs it on phones that need it)
#
# Paths are the distribution's: clients in /usr/bin, the helper in
# /usr/libexec/routedroid, units in /usr/lib/systemd. Packages require the
# newest glibc the binaries link against, so build on the oldest distribution
# they should run on (the release workflow uses Ubuntu 22.04). Uses nfpm, or
# its docker image when nfpm is not installed.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
HOST=$(cd "$HERE/.." && pwd)
BINS=(routedroid routedroidd routedroid-tui routedroid-helper)
if [[ -n ${ROUTEDROID_APK:-} ]]; then
    ROUTEDROID_APK=$(realpath "$ROUTEDROID_APK") && export ROUTEDROID_APK
    echo "routedroidd carries the app: $ROUTEDROID_APK"
else
    echo "warning: routedroidd carries no app (ROUTEDROID_APK=signed APK to embed one)" >&2
fi
[[ -n ${NO_BUILD:-} ]] || (cd "$HOST" && cargo build --release --locked \
    -p routedroid -p routedroidd -p routedroid-tui -p routedroid-helper)

STAGE=$HOST/target/packaging
OUT=$HOST/target/packages
rm -rf "$STAGE" && mkdir -p "$STAGE/bin" "$STAGE/units" "$OUT"
for bin in "${BINS[@]}"; do
    install -m 0755 "$HOST/target/release/$bin" "$STAGE/bin/$bin"
    strip "$STAGE/bin/$bin"
done
for unit in routedroid-helper/systemd/routedroid-helper.socket \
    routedroid-helper/systemd/routedroid-helper@.service routedroidd/systemd/routedroid.service; do
    sed -e 's|@BINDIR@|/usr/bin|g' -e 's|@LIBEXECDIR@|/usr/libexec/routedroid|g' \
        "$HOST/$unit" > "$STAGE/units/${unit##*/}"
done
# Completions and man pages, from the binaries' own command-line definitions.
SHARE=$STAGE/share
mkdir -p "$SHARE/bash-completion/completions" "$SHARE/zsh/site-functions" \
    "$SHARE/fish/vendor_completions.d" "$SHARE/man/man1"
"$STAGE/bin/routedroid" completions bash > "$SHARE/bash-completion/completions/routedroid"
"$STAGE/bin/routedroid" completions zsh > "$SHARE/zsh/site-functions/_routedroid"
"$STAGE/bin/routedroid" completions fish > "$SHARE/fish/vendor_completions.d/routedroid.fish"
"$STAGE/bin/routedroid" manpages "$SHARE/man/man1"
"$STAGE/bin/routedroid-tui" --manpage > "$SHARE/man/man1/routedroid-tui.1"
"$STAGE/bin/routedroidd" --manpage > "$SHARE/man/man1/routedroidd.1"
gzip -9n "$SHARE"/man/man1/*.1

VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$HOST/Cargo.toml")
GLIBC=$(for bin in "${BINS[@]}"; do objdump -T "$STAGE/bin/$bin"; done \
    | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
case $(uname -m) in
    x86_64) ARCH=amd64 ;;
    aarch64) ARCH=arm64 ;;
    *) ARCH=$(uname -m) ;;
esac
export VERSION GLIBC ARCH
echo "routedroid $VERSION ($ARCH), needs glibc >= $GLIBC"

nfpm() {
    if type -P nfpm > /dev/null; then
        (cd "$HERE" && command nfpm "$@")
    else
        docker run --rm -u "$(id -u):$(id -g)" -e VERSION -e GLIBC -e ARCH \
            -v "$HOST/..:/src" -w /src/host/packaging goreleaser/nfpm "$@"
    fi
}
for format in deb rpm; do
    nfpm package --config nfpm.yaml --packager "$format" --target ../target/packages 2>&1 | sed '/^using/d'
done

# The tarball: install.sh with what it installs, laid out as in host/.
TAR=routedroid-$VERSION-linux-$(uname -m)
mkdir -p "$STAGE/$TAR/routedroid-helper" "$STAGE/$TAR/routedroidd"
cp -r "$STAGE/bin" "$SHARE" "$HOST/install.sh" "$HOST/../README.md" "$STAGE/$TAR/"
cp -r "$HOST/routedroid-helper/helper.toml" "$HOST/routedroid-helper/systemd" "$STAGE/$TAR/routedroid-helper/"
cp -r "$HOST/routedroidd/systemd" "$STAGE/$TAR/routedroidd/"
tar -C "$STAGE" --owner=0 --group=0 --sort=name -czf "$OUT/$TAR.tar.gz" "$TAR"
echo "created package: ../target/packages/$TAR.tar.gz"
