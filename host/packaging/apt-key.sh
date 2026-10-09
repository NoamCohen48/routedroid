#!/usr/bin/env bash
# Make the apt repository's signing key, once, and hand it to the docs
# workflow as this repository's APT_SIGNING_KEY secret (needs gpg and gh).
#
#   host/packaging/apt-key.sh [KEY]   (default ~/.config/routedroid/apt-signing.asc)
#
# Back the key up. Users trust the repository by this key: a new one means
# every user fetches routedroid.gpg again before apt takes an update.
#
# Given a KEY that exists, it is not touched; the secret is set again from it.
set -euo pipefail
KEY=${1:-$HOME/.config/routedroid/apt-signing.asc}
command -v gpg > /dev/null || { echo "needs gpg"; exit 2; }
command -v gh > /dev/null || { echo "needs gh, logged in to the repository"; exit 2; }

if [[ ! -e $KEY ]]; then
    mkdir -p "$(dirname "$KEY")"
    GNUPGHOME=$(mktemp -d)
    export GNUPGHOME
    trap 'rm -rf "$GNUPGHOME"' EXIT
    # No passphrase: the workflow signs unattended, and the secret store is
    # what guards it.
    gpg --batch --quiet --passphrase '' --quick-gen-key \
        "Routedroid apt repository" ed25519 sign never
    (umask 077 && gpg --batch --armor --export-secret-keys > "$KEY")
    echo "made $KEY: back it up"
fi
grep -q 'BEGIN PGP PRIVATE KEY BLOCK' "$KEY" || { echo "$KEY is not an armored secret key"; exit 2; }
gh secret set APT_SIGNING_KEY < "$KEY"
echo "the docs workflow now signs the apt repository with $KEY"
