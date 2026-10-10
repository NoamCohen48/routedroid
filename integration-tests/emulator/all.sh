#!/usr/bin/env bash
# Every emulator rig, one after another, on one emulator: what CI's e2e
# workflow runs, and what to run by hand before a change goes up. Installs
# the debug app first. Each rig prints its own checks; this prints which
# rigs failed and exits 1 if any did.
#
#   integration-tests/emulator/all.sh [SERIAL]     (emulator-5554 by default)
#
# Needs what the rigs need (README.md): the release binaries in
# host/target/release, android/app/build/outputs/apk/debug/app-debug.apk,
# socat, unshare and nsenter, and unprivileged user namespaces.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SERIAL=${1:-emulator-5554}
APK=$HERE/../../android/app/build/outputs/apk/debug/app-debug.apk
adb -s "$SERIAL" install -r "$APK" > /dev/null || { echo "could not install $APK"; exit 2; }

# A phone just booted is busy for minutes (system apps starting, optimizing,
# even timing out), and the first session's steps would time out with it.
# Wait until the app opens within 5 s, a few times running.
settle() {
    local launcher quick=0 started=$SECONDS t
    launcher=$(adb -s "$SERIAL" shell cmd package resolve-activity --brief \
        -c android.intent.category.LAUNCHER dev.routedroid | tail -1 | tr -d '\r')
    while ((quick < 3)); do
        if ((SECONDS - started > 600)); then echo "the phone did not settle in 10 min"; return 1; fi
        t=$SECONDS
        if timeout 30 adb -s "$SERIAL" shell am start -W -n "$launcher" > /dev/null 2>&1 &&
            ((SECONDS - t < 5)); then quick=$((quick + 1)); else quick=0; sleep 5; fi
        adb -s "$SERIAL" shell am force-stop dev.routedroid
    done
    echo "the phone settled after $((SECONDS - started)) s"
}
settle || exit 2

failed=()
rig() { # rig NAME CMD...: run one rig, remember it if it fails
    echo "=== $1"
    shift
    "$@" || failed+=("$*")
}
rig "a session ended by Ctrl-C"        "$HERE/userns.sh" "$SERIAL" sigint
rig "a session ended by the app"       "$HERE/userns.sh" "$SERIAL" app
rig "a session stopped right away"     "$HERE/userns.sh" "$SERIAL" early
rig "squatters on the app port"        env SQUAT=3 "$HERE/userns.sh" "$SERIAL" sigint
rig "a misbehaving host"               python3 "$HERE/fake_host.py" "$SERIAL" all
rig "a hostile app"                    "$HERE/hostile.sh" "$SERIAL"

if ((${#failed[@]})); then
    printf 'FAILED: %s\n' "${failed[@]}"
    exit 1
fi
echo "all emulator rigs passed"
