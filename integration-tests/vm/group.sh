#!/usr/bin/env bash
# Joining group routedroid needs no new login, on the `ubuntu` guest: a
# user's daemon started before they joined is refused, with what to do, and
# is admitted the moment `usermod` adds them, though its own groups still
# lack the group. No phone needed.
#
#   ./lab.sh up router ubuntu && ./group.sh
#
# The guest needs Routedroid installed and lan0 allowed (as cli.sh leaves
# it); a .deb in host/target/packages is installed first, as an upgrade.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
rig_tmp group
pc() { timeout 60 "$HERE/lab.sh" ssh ubuntu "$@"; }
RUN=/tmp/fresh-run
# fresh ARGS: `routedroid ARGS` as user fresh, against their own daemon.
fresh() { pc "sudo -u fresh env XDG_RUNTIME_DIR=$RUN routedroid $* 2>&1; echo exit \$?" > "$S/out"; }
says() { fresh "${@:2}"; grep -qF -- "$1" "$S/out" && return 0; cat "$S/out"; return 1; }

DEB=$(ls "$HERE"/../../host/target/packages/routedroid_*_amd64.deb 2>/dev/null)
if [[ -n $DEB ]]; then
    pc 'cat > /tmp/routedroid.deb' < "$DEB"
    pc 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall /tmp/routedroid.deb' > "$S/install" 2>&1
fi
check "anyone may connect to the socket"     eval "pc stat -c %a /run/routedroid/helper.sock | grep -qx 666"

# gone: no user fresh, nor anything of theirs running.
gone() {
    pc "sudo pkill -u fresh; while pgrep -u fresh > /dev/null; do sleep 0.2; done
        sudo userdel -r fresh; sudo rm -rf $RUN; ! id fresh" > /dev/null 2>&1
}
check "no user fresh to begin with"          gone
pc "sudo useradd -m fresh"
pc "sudo install -d -o fresh -m 0700 $RUN"
pc "sudo -u fresh setsid -f sh -c 'echo \$\$ > $RUN/pid; exec env XDG_RUNTIME_DIR=$RUN routedroidd --notify false > $RUN/log 2>&1 < /dev/null'"
slowly() { local _; for _ in $(seq 1 15); do "$@" && return 0; sleep 1; done; return 1; }
check "a non-member's daemon starts"         slowly pc "sudo test -S $RUN/routedroid/control.sock"
check "the helper refuses it, saying why"    says "is not in group routedroid; \`sudo routedroid setup\` adds it" interfaces
check "doctor says the same"                 says "is not in group routedroid" doctor

pc 'sudo usermod -aG routedroid fresh'
GID=$(pc getent group routedroid | cut -d: -f3)
PID=$(pc sudo cat $RUN/pid)
lacks() { [[ -n $PID ]] && pc grep '^Groups:' "/proc/$PID/status" > "$S/groups" && ! grep -qw "$GID" "$S/groups"; }
check "the daemon still lacks the group"     lacks
check "but is admitted at once"              says "lan0 " interfaces
check "doctor agrees"                        says "helper: " doctor
check "with nothing to complain of"          eval "! grep -q 'not in group' '$S/out'"

check "and none left"                        gone
rig_end
