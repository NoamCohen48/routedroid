#!/usr/bin/env bash
# The packages in throwaway containers of the distributions they target: no
# systemd and no phone, so this is the packaging itself. Dependencies
# resolve, the scripts create the group and skip systemd, the binaries run
# (glibc), an upgrade keeps an edited policy, and removal leaves no file or
# directory behind. The tarball's install.sh gets the same life (into
# /usr/local) on each. The real lifecycle, with a phone, is ../vm/package.sh.
#
#   host/packaging/build.sh && integration-tests/packages/containers.sh
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
PKGS=$(cd "$HERE/../../host/target/packages" && pwd)
rig_tmp packages

# inside IMAGE SCRIPT [LOG]: run SCRIPT as root in a fresh IMAGE, the packages in /p.
# NET_ADMIN, in the container's own network namespace: removal's doctor --repair reads nft.
inside() {
    docker run --rm --cap-add NET_ADMIN -v "$PKGS:/p:ro" "$1" bash -euc "$2" > "$S/${3:-$1}.log" 2>&1
}
# The same life on each: install, edit the policy, upgrade, remove.
life() { # life INSTALL UPGRADE REMOVE OWNED-FILES
    cat <<SH
$1
getent group routedroid
routedroid --version && routedroidd --version && /usr/libexec/routedroid/routedroid-helper --version
ls /usr/share/bash-completion/completions/routedroid /usr/share/fish/vendor_completions.d/routedroid.fish \
    /usr/share/zsh/*/_routedroid
echo '# edited' >> /etc/routedroid/helper.toml
$2
grep -q '# edited' /etc/routedroid/helper.toml
$3
! ls -d /usr/bin/routedroid* /usr/libexec/routedroid /usr/lib/systemd/system/routedroid-helper* \
    /usr/lib/systemd/user/routedroid.service /usr/share/doc/routedroid /usr/share/man/man1/routedroid* \
    /usr/share/*/*/*routedroid* 2>/dev/null || exit 1  # set -e ignores !
$4
SH
}
# Container images leave out man pages when installing, so those are checked in the package.
man='routedroid.1.gz routedroid-start.1.gz routedroid-tui.1.gz routedroidd.1.gz'
deb=$(life "apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends /p/*.deb
    for page in $man; do dpkg -c /p/*.deb | grep -q usr/share/man/man1/\$page; done" \
    'dpkg -i /p/*.deb' 'dpkg --purge routedroid' '! test -e /etc/routedroid && ! getent group routedroid')
rpm=$(life "dnf install -y -q --setopt=install_weak_deps=False /p/*.rpm
    for page in $man; do rpm -qlp /p/*.rpm | grep -q /usr/share/man/man1/\$page; done" \
    'rpm -Uvh --force /p/*.rpm' 'rpm -e routedroid' 'test -f /etc/routedroid/helper.toml.rpmsave')
# The tarball brings no dependencies: nftables is the user's to install.
tarball=$(cat <<'SH'
tar -C /tmp -xzf /p/routedroid-*-linux-*.tar.gz
/tmp/routedroid-*/install.sh
getent group routedroid && test -f /etc/systemd/system/routedroid-helper@.service
routedroid --version && routedroidd --version && /usr/local/libexec/routedroid/routedroid-helper --version
ls /usr/local/share/bash-completion/completions/routedroid /usr/local/share/zsh/site-functions/_routedroid \
    /usr/local/share/fish/vendor_completions.d/routedroid.fish /usr/local/share/man/man1/routedroid-start.1.gz \
    /usr/local/share/man/man1/routedroid-tui.1.gz /usr/local/share/man/man1/routedroidd.1.gz
echo '# edited' >> /etc/routedroid/helper.toml
/tmp/routedroid-*/install.sh
grep -q '# edited' /etc/routedroid/helper.toml
/tmp/routedroid-*/install.sh --uninstall --purge
! ls -d /usr/local/bin/routedroid* /usr/local/libexec/routedroid /etc/systemd/system/routedroid-helper* \
    /etc/systemd/user/routedroid.service /etc/routedroid /usr/local/share/man/man1/routedroid* \
    /usr/local/share/*/*/*routedroid* 2>/dev/null || exit 1
! getent group routedroid || exit 1
SH
)
for image in debian:trixie ubuntu:24.04; do
    check "$image: install, upgrade, purge" inside "$image" "$deb"
done
check "fedora: install, upgrade, erase" inside fedora:latest "$rpm"
apt='apt-get update -qq && apt-get install -y -qq nftables > /dev/null'
for image in debian:trixie ubuntu:24.04; do
    check "$image: the tarball's install.sh, again, --uninstall --purge" \
        inside "$image" "$apt; $tarball" "$image-tarball"
done
check "fedora: the tarball's install.sh, again, --uninstall --purge" \
    inside fedora:latest "dnf install -y -q nftables; $tarball" fedora-tarball
rig_end
