#!/usr/bin/env bash
# An apt repository of the given .debs in OUT, laid out for a static host
# (the docs site's apt/ on GitHub Pages):
#
#   OUT/pool/main/*.deb
#   OUT/dists/stable/{Release,InRelease,Release.gpg}, main/binary-ARCH/Packages{,.gz}
#   OUT/routedroid.gpg          the public key, for apt's signed-by
#
#   APT_SIGNING_KEY="$(cat key.asc)" host/packaging/apt-repo.sh OUT DEB...
#
# APT_SIGNING_KEY is an armored secret key without a passphrase, as
# apt-key.sh makes. Needs apt-ftparchive (apt-utils) and gpg.
set -euo pipefail
OUT=${1:?usage: apt-repo.sh OUT DEB...}; shift
[[ $# -gt 0 ]] || { echo "no .deb given"; exit 2; }
: "${APT_SIGNING_KEY:?an armored secret key, as apt-key.sh makes}"
mkdir -p "$OUT/pool/main"
cp -- "$@" "$OUT/pool/main/"
cd "$OUT"
mapfile -t archs < <(for deb in pool/main/*.deb; do dpkg-deb -f "$deb" Architecture; done | sort -u)
for arch in "${archs[@]}"; do
    dir=dists/stable/main/binary-$arch
    mkdir -p "$dir"
    apt-ftparchive --arch "$arch" packages pool > "$dir/Packages"
    gzip -9kf "$dir/Packages"
done
release=(-o APT::FTPArchive::Release::Origin=Routedroid -o APT::FTPArchive::Release::Label=Routedroid
    -o APT::FTPArchive::Release::Suite=stable -o APT::FTPArchive::Release::Codename=stable
    -o APT::FTPArchive::Release::Components=main
    -o "APT::FTPArchive::Release::Architectures=${archs[*]}")
# Written aside first: apt-ftparchive would list a Release it is writing.
apt-ftparchive "${release[@]}" release dists/stable > Release.new
mv Release.new dists/stable/Release

GNUPGHOME=$(mktemp -d)
export GNUPGHOME
trap 'rm -rf "$GNUPGHOME"' EXIT
gpg --batch --quiet --import <<< "$APT_SIGNING_KEY"
gpg --batch --yes --armor --detach-sign -o dists/stable/Release.gpg dists/stable/Release
gpg --batch --yes --clearsign -o dists/stable/InRelease dists/stable/Release
gpg --batch --export > routedroid.gpg
echo "apt repository in $OUT: ${archs[*]}, $(find pool/main -name "*.deb" | wc -l) package(s)"
