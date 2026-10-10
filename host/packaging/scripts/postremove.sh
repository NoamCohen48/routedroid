#!/bin/sh
# deb: remove | purge | upgrade NEW-VERSION | ...; rpm: 0 on erase, 1 on upgrade.
case "$1" in
    remove | purge | 0) ;;
    *) exit 0 ;;
esac
[ -d /run/systemd/system ] && systemctl daemon-reload
rm -rf /run/routedroid
# dpkg removes the policy itself on purge; the state goes only if every
# journal was replayed (nothing but lock files left), and then the group.
if [ "$1" = purge ]; then
    if [ -z "$(find /var/lib/routedroid -type f ! -name .lock 2>/dev/null)" ]; then
        rm -rf /var/lib/routedroid
        groupdel routedroid 2>/dev/null || true
    else
        echo "warning: /var/lib/routedroid still holds journals; kept, and group routedroid too" >&2
    fi
fi
exit 0
