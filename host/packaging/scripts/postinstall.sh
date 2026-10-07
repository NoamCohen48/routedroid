#!/bin/sh
# deb: configure [OLD-VERSION]; rpm: 1 on install, 2 on upgrade.
set -e
getent group routedroid >/dev/null || groupadd --system routedroid
first=0
case "$1" in
    1) first=1 ;;
    configure) [ -z "$2" ] && first=1 ;;
esac
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload
    # On an upgrade the socket stays up (its instances Require it, so a
    # restart would end every session): running sessions keep their helper,
    # and the next connection starts this version's.
    [ "$first" = 1 ] && systemctl enable --now routedroid-helper.socket
    # Each logged-in user's manager reads the new routedroid.service too;
    # their daemons run on until restarted.
    for unit in $(systemctl list-units 'user@*.service' --state=running --no-legend --plain | cut -d' ' -f1); do
        uid=${unit#user@}; uid=${uid%.service}
        user=$(id -nu "$uid" 2>/dev/null) && systemctl --user -M "$user@" daemon-reload 2>/dev/null || true
    done
fi
if [ "$first" = 1 ]; then
    cat <<'MSG'
Routedroid is installed. Phones are attached by members of group routedroid,
on the interfaces /etc/routedroid/helper.toml allows (none yet):

  sudo usermod -aG routedroid "$USER"     then log out completely (or reboot)
  routedroid interfaces                   then allow one in /etc/routedroid/helper.toml
  systemctl --user enable --now routedroid
  routedroid doctor
MSG
fi
