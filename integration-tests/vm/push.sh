#!/usr/bin/env bash
# Build Routedroid here, copy what install.sh needs into the `host` guest and
# run it there as root, the way an operator would.
#
#   ./push.sh            FEATURES=testing ./push.sh  (the crash hook, for kill tests)
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=$(cd "$HERE/../../host" && pwd)
BINS=(routedroid routedroidd routedroid-tui routedroid-helper)
(cd "$SRC" && cargo build --release --locked -q ${FEATURES:+--features "routedroid-helper/$FEATURES"} \
    -p routedroid -p routedroidd -p routedroid-tui -p routedroid-helper)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/host/target/release" "$STAGE/host/routedroid-helper" "$STAGE/host/routedroidd"
for bin in "${BINS[@]}"; do cp "$SRC/target/release/$bin" "$STAGE/host/target/release/"; done
cp "$SRC/install.sh" "$STAGE/host/"
cp -r "$SRC/routedroid-helper/systemd" "$STAGE/host/routedroid-helper/"
cp -r "$SRC/routedroidd/systemd" "$STAGE/host/routedroidd/"
tar -C "$STAGE" -czf - host | "$HERE/lab.sh" ssh host 'rm -rf ~/routedroid && mkdir ~/routedroid && tar -C ~/routedroid -xzf - && sudo ~/routedroid/host/install.sh'
