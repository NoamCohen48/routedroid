#!/usr/bin/env bash
# Hostile-app probe (review A-6): installs android/testing/hostile, lets it attack the
# app's exported surface, then checks the real host can still start a session.
#
#     hostile.sh SERIAL        (the app must be installed)
set -euo pipefail
SERIAL=${1:?usage: hostile.sh SERIAL}
HERE=$(cd "$(dirname "$0")" && pwd)
ANDROID=$HERE/../../android

(cd "$ANDROID" && ./gradlew -q -Proutedroid.hostile :hostile:assembleDebug)
adb -s "$SERIAL" install -r "$ANDROID/testing/hostile/build/outputs/apk/debug/hostile-debug.apk" >/dev/null
"$HERE/prepare-device.sh" "$SERIAL"
adb -s "$SERIAL" logcat -c
adb -s "$SERIAL" shell am start -W -n dev.routedroid.hostile/.HostileActivity >/dev/null

for _ in $(seq 30); do
    adb -s "$SERIAL" logcat -d -s HostileProbe | grep -q HOSTILE-DONE && break
    sleep 1
done
results=$(adb -s "$SERIAL" logcat -d -s HostileProbe | grep -o 'HOSTILE-.*' || true)
echo "$results"
adb -s "$SERIAL" shell am force-stop dev.routedroid.hostile

failed=0
grep -q HOSTILE-DONE <<<"$results" || { echo "probe did not finish"; failed=1; }
grep -Eq 'HOSTILE-RESULT [^ ]+ (FAIL|INCONCLUSIVE)' <<<"$results" && failed=1
# The launch burst must not lock the real host out (A-2.2).
python3 "$HERE/fake_host.py" "$SERIAL" host_stop | grep -E 'PASS|FAIL' || failed=1
echo "RESULT: $([[ $failed == 0 ]] && echo passed || echo failed)"
exit $failed
