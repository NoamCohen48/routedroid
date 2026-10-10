# shellcheck shell=bash
# Shared by the rigs; source it right after `set -u -o pipefail`. Rigs are
# check-style: a failing step is counted and reported, not fatal, so they do
# not use `set -e`.

pass=0; fail=0

# A daemon a rig starts here must not pop up on the desktop of whoever runs it.
export ROUTEDROID_NOTIFY=false

# check NAME CMD...: run CMD, count and print the outcome.
check() {
    local name=$1; shift
    if "$@"; then echo "PASS  $name"; pass=$((pass + 1))
    else echo "FAIL  $name"; fail=$((fail + 1)); fi
}

# rig_tmp NAME: a scratch directory in $S; rig_end removes it on success
# (unless KEEP_TMP=1) and keeps it for inspection on failure.
rig_tmp() {
    S=$(mktemp -d "${TMPDIR:-/tmp}/rd-$1.XXXXXX")
    echo "artifacts: $S"
}

# rig_end: the summary line and the exit status.
rig_end() {
    echo "RESULT: $pass passed, $fail failed"
    if [[ $fail -eq 0 && ${KEEP_TMP:-0} != 1 ]]; then rm -rf "$S"
    else echo "artifacts kept in $S"; fi
    [[ $fail -eq 0 ]]
}

# userns_start: an unprivileged user+network namespace with `lo` up, held
# open by a sleeping process. Sets NSPID and NS, the command prefix that
# enters it (an array: "${NS[@]}" ip ...). Waits until the namespace is
# really there instead of sleeping for a guess.
userns_start() {
    unshare -Urn --propagation unchanged sh -c 'ip link set lo up; exec sleep infinity' &
    NSPID=$!
    NS=(nsenter -t "$NSPID" -U -n --preserve-credentials)
    local _
    for _ in $(seq 1 250); do
        "${NS[@]}" ip -o link show lo 2>/dev/null | grep -q '[<,]UP[,>]' && return 0
        kill -0 "$NSPID" 2>/dev/null || break
        sleep 0.02
    done
    echo "the namespace did not come up" >&2
    return 1
}

# in_ns CMD...: CMD inside the namespace; a function, so `check` and `!` take it.
in_ns() { "${NS[@]}" "$@"; }

# wait_for_socket PATH: until a Unix socket exists at PATH (3 s).
wait_for_socket() {
    local _
    for _ in $(seq 1 30); do [[ -S $1 ]] && return 0; sleep 0.1; done
    return 1
}

# eventually CMD...: CMD succeeds within 3 s (teardown after a controller
# leaves is asynchronous).
eventually() {
    local _
    for _ in $(seq 1 30); do "$@" && return 0; sleep 0.1; done
    return 1
}
