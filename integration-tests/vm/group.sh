#!/usr/bin/env bash
# Joining group routedroid needs no new login, on the `ubuntu` guest: a
# user's daemon started before they joined is refused, with what to do, and
# is admitted the moment `usermod` adds them, though its own groups still
# lack the group. Taken out again, it is refused again; then a `routedroid
# start` at a terminal offers setup, which adds them, and the start goes on
# without a logout. No phone needed.
#
#   ./lab.sh up router ubuntu && ./group.sh
#   GUEST=fedora ./group.sh
#
# The guest needs Routedroid installed and lan0 allowed (as cli.sh or
# package.sh leave it); the guest's package from host/target/packages is
# installed first, as an upgrade.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
rig_tmp group
GUEST=${GUEST:-ubuntu}
pc() { timeout 60 "$HERE/lab.sh" ssh "$GUEST" "$@"; }
RUN=/tmp/fresh-run
# fresh ARGS: `routedroid ARGS` as user fresh, against their own daemon.
fresh() { pc "sudo -u fresh env XDG_RUNTIME_DIR=$RUN routedroid $* 2>&1; echo exit \$?" > "$S/out"; }
says() { fresh "${@:2}"; grep -qF -- "$1" "$S/out" && return 0; cat "$S/out"; return 1; }

PKGS=$HERE/../../host/target/packages
if [[ $GUEST == fedora ]]; then
    PKG=$(ls "$PKGS"/routedroid-*.x86_64.rpm 2>/dev/null)
    INSTALL='if rpm -q routedroid; then sudo dnf reinstall -y /tmp/routedroid.rpm; else sudo dnf install -y /tmp/routedroid.rpm; fi'
else
    PKG=$(ls "$PKGS"/routedroid_*_amd64.deb 2>/dev/null)
    INSTALL='sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall /tmp/routedroid.deb'
fi
if [[ -n $PKG ]]; then
    pc "cat > /tmp/routedroid.${PKG##*.}" < "$PKG"
    pc "$INSTALL" > "$S/install" 2>&1
fi
check "anyone may connect to the socket"     eval "pc stat -c %a /run/routedroid/helper.sock | grep -qx 666"

# gone: no user fresh, nor anything of theirs running.
gone() {
    pc "sudo pkill -u fresh; while pgrep -u fresh > /dev/null; do sleep 0.2; done
        sudo userdel -r fresh; sudo rm -rf $RUN /etc/sudoers.d/fresh; ! id fresh" > /dev/null 2>&1
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

pc 'sudo gpasswd -d fresh routedroid' > /dev/null
check "taken out, it is refused again"       says "is not in group routedroid" interfaces

echo "== a start at a terminal offers setup to a user not in the group"
pc "echo 'fresh ALL=(ALL) NOPASSWD: ALL' | sudo tee /etc/sudoers.d/fresh > /dev/null"
tmux_fresh() { pc "sudo -u fresh env XDG_RUNTIME_DIR=$RUN tmux $*"; }
tmux_fresh "new-session -d -s fresh -x 100 -y 30 'routedroid start -s nosuch --lan-if lan0; echo EXIT=\$?; sleep 600'"
# shows TEXT: fresh's pane has it, within 30 s.
shows() {
    local _
    for _ in $(seq 1 15); do
        tmux_fresh capture-pane -p -t fresh > "$S/screen" && grep -qF -- "$1" "$S/screen" && return 0
        sleep 2
    done
    cat "$S/screen"; return 1
}
check "start says what is missing"           shows "you are not in group routedroid yet"
check "and offers setup"                     shows "Set it up now (sudo routedroid setup)? [Y/n]"
tmux_fresh send-keys -t fresh Enter
check "setup asks about the LAN"             shows "Let phones join the LAN through lan0? [Y/n]"
tmux_fresh send-keys -t fresh Enter; sleep 1; tmux_fresh send-keys -t fresh Enter
check "it added fresh"                       shows "added fresh to group routedroid"
check "and the start went on to the phone"   shows "nosuch is not attached"
check "without asking for a logout"          eval "! grep -qi 'log out' '$S/screen'"
check "exit 2, as for any unknown phone"     shows "EXIT=2"

check "and none left"                        gone
rig_end
