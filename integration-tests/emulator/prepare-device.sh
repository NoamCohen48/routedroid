#!/usr/bin/env bash
# Puts the installed app in the state the rigs assume, so no system dialog waits for a tap:
# VPN consent granted, notifications allowed (a runtime permission from API 33), and no
# leftover prompt or session from an earlier run.
#
#     prepare-device.sh SERIAL
set -u
SERIAL=${1:?usage: prepare-device.sh SERIAL}
a() { adb -s "$SERIAL" "$@"; }
a shell am force-stop dev.routedroid
a shell am force-stop com.android.vpndialogs
a shell appops set dev.routedroid ACTIVATE_VPN allow
[[ $(a shell getprop ro.build.version.sdk | tr -d '\r') -ge 33 ]] &&
    a shell pm grant dev.routedroid android.permission.POST_NOTIFICATIONS
a shell input keyevent KEYCODE_HOME
true
