#!/usr/bin/env bash
# The apt repository host/packaging/apt-repo.sh builds, used as a person
# would: in throwaway Debian and Ubuntu containers, signed with a throwaway
# key, added with signed-by (as a file: source; GitHub Pages serves the same
# tree), then `apt install routedroid`. A repository the key did not sign
# is refused.
#
#   host/packaging/build.sh && integration-tests/packages/apt-repo.sh
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
ROOT=$(cd "$HERE/../.." && pwd)
rig_tmp apt-repo

# shellcheck disable=SC2016 # expanded in the container
life='
apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq apt-utils gpg > /dev/null
key() { GNUPGHOME=$(mktemp -d) sh -c "gpg --batch --quiet --passphrase \"\" --quick-gen-key \"$1\" ed25519 sign never && gpg --batch --armor --export-secret-keys"; }
APT_SIGNING_KEY=$(key test) /src/host/packaging/apt-repo.sh /repo /p/*.deb
mkdir -p /etc/apt/keyrings && cp /repo/routedroid.gpg /etc/apt/keyrings/routedroid.gpg
echo "deb [signed-by=/etc/apt/keyrings/routedroid.gpg] file:/repo stable main" > /etc/apt/sources.list.d/routedroid.list
apt-get update 2>&1 | tee /tmp/update
! grep -qi "not signed\|NO_PUBKEY\|BADSIG\|^W:" /tmp/update
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends routedroid
routedroid --version
echo "== signed by another key"
APT_SIGNING_KEY=$(key other) /src/host/packaging/apt-repo.sh /forged /p/*.deb
echo "deb [signed-by=/etc/apt/keyrings/routedroid.gpg] file:/forged stable main" > /etc/apt/sources.list.d/routedroid.list
if apt-get update > /tmp/forged 2>&1; then cat /tmp/forged; exit 1; fi
grep -q "NO_PUBKEY\|not signed\|signatures were invalid" /tmp/forged
'
inside() { # inside IMAGE: the life above in a fresh IMAGE
    docker run --rm -v "$ROOT/host/target/packages:/p:ro" -v "$ROOT:/src:ro" "$1" bash -euc "$life" \
        > "$S/${1/:/-}.log" 2>&1
}
for image in debian:trixie ubuntu:24.04; do
    check "$image: apt install from the repository, and a forged one refused" inside "$image"
done
rig_end
