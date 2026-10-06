#!/usr/bin/env bash
# The packages in throwaway containers of the distributions they target: no
# systemd and no phone, so this is the packaging itself. Dependencies
# resolve, the scripts create the group and skip systemd, the binaries run
# (glibc), an upgrade keeps an edited policy, and removal leaves no file or
# directory behind. The real lifecycle, with a phone, is ../vm/package.sh.
#
#   host/packaging/build.sh && integration-tests/packages/containers.sh
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
PKGS=$(cd "$HERE/../../host/target/packages" && pwd)
rig_tmp packages

# inside IMAGE SCRIPT: run SCRIPT as root in a fresh IMAGE, the packages in /p.
inside() { docker run --rm -v "$PKGS:/p:ro" "$1" bash -euc "$2" > "$S/$1.log" 2>&1; }
# The same life on each: install, edit the policy, upgrade, remove.
life() { # life INSTALL UPGRADE REMOVE OWNED-FILES
    cat <<SH
$1
getent group routedroid
routedroid --version && routedroidd --version && /usr/libexec/routedroid/routedroid-helper --version
echo '# edited' >> /etc/routedroid/helper.toml
$2
grep -q '# edited' /etc/routedroid/helper.toml
$3
! ls -d /usr/bin/routedroid* /usr/libexec/routedroid /usr/lib/systemd/system/routedroid-helper* \
    /usr/lib/systemd/user/routedroid.service /usr/share/doc/routedroid 2>/dev/null || exit 1  # set -e ignores !
$4
SH
}
deb=$(life 'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends /p/*.deb' \
    'dpkg -i /p/*.deb' 'dpkg --purge routedroid' '! test -e /etc/routedroid && ! getent group routedroid')
rpm=$(life 'dnf install -y -q --setopt=install_weak_deps=False /p/*.rpm' \
    'rpm -Uvh --force /p/*.rpm' 'rpm -e routedroid' 'test -f /etc/routedroid/helper.toml.rpmsave')
for image in debian:trixie ubuntu:24.04; do
    check "$image: install, upgrade, purge" inside "$image" "$deb"
done
check "fedora: install, upgrade, erase" inside fedora:latest "$rpm"
rig_end
