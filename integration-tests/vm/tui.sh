#!/usr/bin/env bash
# The TUI as a person uses it, on the `ubuntu` guest: it runs in a tmux pane
# of 80x24 (the smallest usual terminal), keys go in with send-keys and the
# screen is read back with capture-pane. Connect through the form, watch it
# go active, pull the phone (reconnecting, still on screen), put it back,
# disconnect.
#
#   PHONE=04e8:6860 ./lab.sh up router ubuntu && PHONE=04e8:6860 ./tui.sh SERIAL
#   TUI=/tmp/routedroid-tui ...      (another build, copied into the guest)
set -u -o pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source-path=SCRIPTDIR source=../lib.sh
source "$HERE/../lib.sh"
SERIAL=${1:?usage: PHONE=vendor:product tui.sh SERIAL}
: "${PHONE:?PHONE=vendor:product, to pull the phone out}"
TUI=${TUI:-routedroid-tui}
rig_tmp tui
pc() { timeout 60 "$HERE/lab.sh" ssh ubuntu "$@"; }
key() { pc tmux send-keys -t tui "$@"; sleep 1; }
# shows TEXT: the screen has it, within 60 s; the last screen is kept in $S.
shows() {
    local _
    for _ in $(seq 1 30); do
        pc tmux capture-pane -p -t tui > "$S/screen" && grep -qF -- "$1" "$S/screen" && return 0
        sleep 2
    done
    cat "$S/screen"; return 1
}

pc "routedroid stop -s $SERIAL >/dev/null 2>&1; tmux kill-server 2>/dev/null; true"
pc "tmux new-session -d -s tui -x 80 -y 24 'TERM=xterm-256color $TUI'"
echo "== 80x24"
check "the phone is listed"                shows "> $SERIAL"
check "with the keys that matter"          shows "s connect  x disconnect  q quit"
key s
check "s opens the connect form"           shows "Connect $SERIAL"
check "with the reconnect wait"            shows "Wait for an unplugged phone"
key Enter
check "Enter connects it"                  shows "state: active"
check "traffic in readable units"          eval "shows ' kB' || shows ' MB'"
echo "== unplugged and back"
PHONE=$PHONE "$HERE/lab.sh" unplug ubuntu > /dev/null
check "the phone stays on screen, gone"    shows "gone         no      reconnecting"
check "its address held"                   shows "address is held"
PHONE=$PHONE "$HERE/lab.sh" plug ubuntu > /dev/null
check "back, and active again"             shows "device       yes     active"
echo "== disconnect"
key x
check "x asks first"                       shows "Disconnect $SERIAL? [y/N]"
key y
check "y ends it"                          shows "$SERIAL: ended: stopped"
check "and the log says so once"           eval "[ \"\$(grep -c '$SERIAL: .*stopped' '$S/screen')\" = 1 ]"
key q
check "q quits"                            eval "! pc tmux has-session -t tui 2>/dev/null"
rig_end
