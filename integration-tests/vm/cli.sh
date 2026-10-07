#!/usr/bin/env bash
# The CLI as a person uses it, on the `ubuntu` guest with a real phone: what
# it says and the exit code it gives, for the mistakes people make and for
# a connection followed in the foreground (in tmux, so Ctrl-C is a real
# keypress), detached, unplugged and back, and unplugged for good.
#
#   PHONE=04e8:6860 ./lab.sh up router ubuntu && PHONE=04e8:6860 ./cli.sh SERIAL
#
# The guest needs Routedroid installed and lan0 allowed by DHCP (as
# app-install.sh leaves it); a .deb in host/target/packages is installed first.
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: PHONE=vendor:product cli.sh SERIAL}
: "${PHONE:?PHONE=vendor:product, to pull the phone out}"
rig_tmp cli
pc() { timeout 120 "$HERE/lab.sh" ssh ubuntu "$@"; }
# says EXIT TEXT -- ARGS: `routedroid ARGS` exits EXIT and prints TEXT.
says() {
    local want=$1 text=$2; shift 3
    pc "routedroid $* > /tmp/out 2>&1; echo \"exit \$?\" >> /tmp/out; cat /tmp/out" > "$S/out"
    if grep -qF -- "$text" "$S/out" && tail -1 "$S/out" | grep -qx "exit $want"; then return 0; fi
    cat "$S/out"; return 1
}
# shows TEXT: the tmux pane has it, within 60 s.
shows() {
    local _
    for _ in $(seq 1 30); do
        pc tmux capture-pane -p -t cli > "$S/screen" && grep -qF -- "$1" "$S/screen" && return 0
        sleep 2
    done
    cat "$S/screen"; return 1
}
slowly() { local _; for _ in $(seq 1 30); do "$@" && return 0; sleep 2; done; return 1; }
status() { pc routedroid status | grep -qF -- "$1"; }
foreground() { # foreground ARGS: `routedroid start ARGS` in a fresh tmux pane
    pc "tmux kill-server 2>/dev/null; tmux new-session -d -s cli -x 100 -y 30 'routedroid start $*; echo EXIT=\$?; sleep 600'"
}
plug() { PHONE=$PHONE "$HERE/lab.sh" "$1" ubuntu > /dev/null; }

DEB=$(ls "$HERE"/../../host/target/packages/routedroid_*_amd64.deb 2>/dev/null)
if [[ -n $DEB ]]; then
    pc 'cat > /tmp/routedroid.deb' < "$DEB"
    pc 'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall /tmp/routedroid.deb' > "$S/install" 2>&1
    pc 'systemctl --user restart routedroid'
fi
pc "routedroid stop -s $SERIAL >/dev/null 2>&1; tmux kill-server 2>/dev/null; true"
check "no reload warning after an upgrade"   eval "! grep -q 'changed on disk' '$S/install' 2>/dev/null"

echo "== setup, on a PC set up already"
setup() {
    if pc "sudo routedroid setup --yes" > "$S/setup" 2>&1 && grep -qF -- "$1" "$S/setup"; then return 0; fi
    cat "$S/setup"; return 1
}
check "setup --yes changes nothing"          setup "already set up for"
check "mgmt0 stays out of the policy"        eval "pc routedroid interfaces | grep -q '^mgmt0 .*not in the helper policy'"
check "completions are installed"            pc test -s /usr/share/bash-completion/completions/routedroid
check "and the man pages"                    pc man -w routedroid-start

echo "== asking"
check "doctor: one phone"                    says 0 "adb: 1 phone ready" -- doctor
check "devices lists it"                     says 0 "$SERIAL" -- devices
check "help speaks plainly"                  eval "! pc routedroid start --help | grep -qi 'decision [0-9]\|gate [0-9]'"

echo "== mistakes, refused at once (exit 2)"
check "an unknown phone"                     says 2 "nosuch is not attached" -- start -s nosuch --lan-if lan0
check "a link outside the policy"            says 2 "mgmt0: not in the helper policy (allow it in /etc/routedroid/helper.toml)" -- start -s "$SERIAL" --lan-if mgmt0
check "an address outside it"                says 2 "10.9.9.9 is not a phone address the policy allows on lan0 (it allows DHCP only)" -- start -s "$SERIAL" --lan-if lan0 --phone-ip 10.9.9.9
check "a bad duration"                       says 2 "invalid value 'banana'" -- start -s "$SERIAL" --lan-if lan0 --reconnect-wait banana
check "stopping what is not running"         says 2 "$SERIAL is not connected" -- stop -s "$SERIAL"
check "nothing was started"                  says 0 "no connections" -- status

echo "== in the foreground"
foreground "-s $SERIAL --lan-if lan0"
check "it says where the phone goes"         shows "started $SERIAL on lan0"
check "and when it is active"                shows "active"
pc tmux send-keys -t cli C-c
check "Ctrl-C stops it, exit 0"              shows "EXIT=0"

echo "== detached, unplugged and back"
pc "(timeout 90 routedroid events > /tmp/events 2>&1 &)"
check "--detach returns at once"             says 0 "started $SERIAL on lan0" -- start -s "$SERIAL" --lan-if lan0 --reconnect-wait 30s --detach
check "a second start is refused"            says 2 "$SERIAL is already connected" -- start -s "$SERIAL" --lan-if lan0
check "it goes active"                       slowly status " active "
plug unplug
check "unplugged: reconnecting"              slowly status " reconnecting "
plug plug
check "back: active again"                   slowly status " active "
check "stop"                                 says 0 "$SERIAL: stopped" -- stop -s "$SERIAL"
check "events read as lines"                 eval "pc cat /tmp/events | grep -q '^[0-9:]\{8\} $SERIAL: active'"
check "including the phone going away"       eval "pc cat /tmp/events | grep -q 'phones attached: none'"
check "--json status, for scripts"          says 0 "[]" -- --json status

echo "== unplugged for good"
foreground "-s $SERIAL --lan-if lan0 --reconnect-wait 10s"
check "active"                               shows "active"
plug unplug
check "it waits the time asked"              shows "held for up to 10 s"
check "then ends, exit 10 (adb)"             shows "EXIT=10"
check "saying why"                           shows "was not back within 10 s"
plug plug
check "nothing left behind"                  slowly says 0 "leftovers: nothing left behind" -- doctor
pc "tmux kill-server 2>/dev/null; true"

echo "== without saying which"
check "the phone is back"                    slowly says 0 "adb: 1 phone ready" -- doctor
check "start takes the one phone and LAN"    says 0 "started $SERIAL on lan0" -- start --detach
check "it goes active"                       slowly status " active "
check "stop takes the one connection"        says 0 "$SERIAL: stopped" -- stop

echo "== a phone that waits on its screen"
key() { pc "adb -s $SERIAL shell input keyevent $1"; }
key KEYCODE_SLEEP
foreground ""
check "off: wake it and unlock it"           shows "the phone's screen is off: wake it and unlock it"
key KEYCODE_WAKEUP
pc "adb -s $SERIAL shell wm dismiss-keyguard"
check "woken and unlocked, it goes on"       shows "active"
pc tmux send-keys -t cli C-c
check "Ctrl-C stops it"                      shows "EXIT=0"
key KEYCODE_SLEEP
sleep 1
key KEYCODE_WAKEUP
foreground ""
check "at the lock screen: unlock it"        shows "the phone is locked: unlock it to continue"
pc "adb -s $SERIAL shell wm dismiss-keyguard"
check "unlocked, it goes on"                 shows "active"
pc tmux send-keys -t cli C-c
check "Ctrl-C stops it again"                shows "EXIT=0"
pc "tmux kill-server 2>/dev/null; true"

echo "== remembered, by name"
check "remember it"                          says 0 "remembered lab ($SERIAL): it connects whenever it is plugged in" -- remember "$SERIAL" --name lab
check "phones lists it"                      says 0 "lab" -- phones
plug unplug
plug plug
check "plugged in: it connects by itself"    slowly status " active "
check "status names it"                      eval "pc routedroid status | grep -q ' lab '"
check "stop by name"                         says 0 "lab: stopped" -- stop lab
check "forget it"                            says 0 "forgot lab ($SERIAL)" -- forget lab
check "nothing is remembered"                eval "! pc routedroid phones | grep -q lab"

echo "== without the daemon"
pc 'systemctl --user stop routedroid'
check "exit 3, with what to do"              says 3 "systemctl --user start routedroid" -- status
pc 'systemctl --user start routedroid'
rig_end
