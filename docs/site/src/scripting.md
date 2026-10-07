# Scripting: JSON and the control API

## --json

Every command takes `--json` and prints one JSON document per answer instead of text:

```sh
routedroid --json status
routedroid --json devices | jq -r '.[] | select(.state == "device") | .serial'
```

`routedroid --json events` prints one JSON line per event, until Ctrl-C:

```json
{"event":"connection","serial":"R58M","state":{"state":"handshaking"}}
{"event":"devices","devices":[]}
```

Check the output of `--json` on your version before relying on a field. The API is versioned
(below), and `routedroid version` prints both versions.

Scripts can also branch on the [exit code](exit-codes.md), which says why a command failed.

## Starting from a script

```sh
routedroid start pixel --detach || exit
# Wait until the phone is active, or give up when its connection ends.
routedroid --json events | jq -r --unbuffered '
    select(.event == "connection") | .state.state' | while read -r state; do
    case $state in
        active) echo "on the LAN"; break ;;
        ended) echo "it ended"; exit 1 ;;
    esac
done
```

`start --detach` returns once the daemon has checked and accepted the request. A refused
start, such as one the policy forbids, fails right there with exit code 2, and nothing is
started.

## The control API

The CLI and the TUI are clients of the daemon's control API, and any program can be one.

- **Transport:** a Unix stream socket owned by you (mode 0600), at
  `$XDG_RUNTIME_DIR/routedroid/control.sock`. `--socket` and `ROUTEDROID_SOCKET` override it.
- **Framing:** newline-delimited JSON.
- **Requests** carry an `id` and a `type`. The daemon answers each with a response carrying
  the same `id`.
- **Events:** after a `subscribe` request, the daemon also sends events at any time.

```text
→ {"id":1,"type":"version"}
← {"msg":"response","id":1,"type":"version","daemon":"0.1.0","api":3}
→ {"id":2,"type":"start","serial":"R58M","lan_if":"eno1"}
← {"msg":"response","id":2,"type":"started","serial":"R58M","lan_if":"eno1","tun":"phone0"}
→ {"id":3,"type":"subscribe"}
← {"msg":"response","id":3,"type":"subscribed"}
← {"msg":"event","event":"connection","serial":"R58M","state":{"state":"handshaking"}}
```

A failed request is answered with `"type":"error"`, a `kind` and a `message`.

| Request | Answer |
|---|---|
| `version` | the daemon's version and API version |
| `devices` | attached phones, whether each can be used, and any connection |
| `interfaces` | the PC's interfaces, and whether a phone may join through each |
| `start` | `started`, once accepted; progress arrives as events |
| `stop` (`serial`: a serial or name) | `stopped`, once the connection has ended, with its outcome |
| `status` | every live connection |
| `phones`, `remember`, `forget` | the [remembered phones](remembered-phones.md) |
| `subscribe` | `subscribed`, then events |
| `doctor` (`repair`: true or false) | the checks |

Every field of `start` is optional: the daemon picks the phone and LAN as `routedroid start`
does. The fields are `serial`, `lan_if`, `phone_ip`, `tun`, `mtu`, `dns` (`"auto"`,
`"none"` or `{"servers": [...]}`), `connect_timeout_secs`, `reconnect_secs` and
`allow_network_adb`.

| Event | Means |
|---|---|
| `connection` | a connection changed state, including its final `ended` |
| `network` | the phone is on the LAN, or its lease was renewed |
| `traffic` | an active connection's counters (at most once a second) |
| `devices` | a phone appeared, went away, or changed adb state |
| `shutdown` | the daemon is stopping every connection |
| `lagged` | this client fell behind and missed events; ask for `status` again |

The exact shapes are pinned by the golden tests in `host/routedroid-ipc/src/golden.rs`. A Rust
program can use the `routedroid-ipc` crate, which the CLI and TUI share.

The API is at version 3. A daemon and a client of different versions refuse each other
(exit code 4): restart the daemon after an upgrade.
