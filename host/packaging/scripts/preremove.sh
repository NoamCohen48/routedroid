#!/bin/sh
# deb: remove | upgrade NEW-VERSION | deconfigure ... | failed-upgrade ...;
# rpm: 0 on erase, 1 on upgrade. Only a removal ends the sessions: replay
# every journal and repair what Routedroid provably left, while the helper
# binary is still here.
case "$1" in
    remove | 0) ;;
    *) exit 0 ;;
esac
if [ -d /run/systemd/system ]; then
    systemctl disable --now routedroid-helper.socket 2>/dev/null || true
    # Waits for every instance, ExecStopPost cleanup included, to finish.
    systemctl stop 'routedroid-helper@*.service' 2>/dev/null || true
fi
helper=/usr/libexec/routedroid/routedroid-helper
if ! "$helper" cleanup || ! "$helper" doctor --repair; then
    echo "warning: something could not be undone; see above (state kept in /var/lib/routedroid)" >&2
fi
echo "a running daemon keeps going until: systemctl --user disable --now routedroid"
exit 0
