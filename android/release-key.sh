#!/usr/bin/env bash
# Make the app's release signing key, once, and hand it to the release
# workflow as this repository's ROUTEDROID_SIGNING_* secrets (needs gh).
#
#   android/release-key.sh [KEYSTORE]   (default ~/.config/routedroid/release.jks)
#
# Back up the keystore and its password file. A phone takes an update only
# when it is signed with the same key: with the key lost, every user has to
# uninstall the app before the next release installs.
#
# Given a KEYSTORE that exists, it is not touched; its secrets are set again
# from it and KEYSTORE.password.
set -euo pipefail
STORE=${1:-$HOME/.config/routedroid/release.jks}
PASSWORD_FILE=$STORE.password
ALIAS=routedroid
command -v keytool > /dev/null || { echo "needs keytool (a JDK)"; exit 2; }
command -v gh > /dev/null || { echo "needs gh, logged in to the repository"; exit 2; }

if [[ ! -e $STORE ]]; then
    mkdir -p "$(dirname "$STORE")"
    (umask 077 && head -c 24 /dev/urandom | base64 > "$PASSWORD_FILE")
    # PKCS12, so the key's password is the store's.
    keytool -genkeypair -keystore "$STORE" -storetype PKCS12 -alias "$ALIAS" \
        -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=Routedroid" \
        -storepass:file "$PASSWORD_FILE" > /dev/null
    chmod 600 "$STORE"
    echo "made $STORE (password in $PASSWORD_FILE): back both up"
fi
[[ -r $PASSWORD_FILE ]] || { echo "no $PASSWORD_FILE for $STORE"; exit 2; }
keytool -list -keystore "$STORE" -alias "$ALIAS" -storepass:file "$PASSWORD_FILE" > /dev/null

PASSWORD=$(cat "$PASSWORD_FILE")
base64 -w0 "$STORE" | gh secret set ROUTEDROID_SIGNING_KEYSTORE
printf %s "$PASSWORD" | gh secret set ROUTEDROID_SIGNING_STORE_PASSWORD
printf %s "$PASSWORD" | gh secret set ROUTEDROID_SIGNING_KEY_PASSWORD
printf %s "$ALIAS" | gh secret set ROUTEDROID_SIGNING_KEY_ALIAS
echo "the release workflow now signs with $STORE"
