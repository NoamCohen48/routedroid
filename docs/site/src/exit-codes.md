# Exit codes

`routedroid` exits with a code that says why it failed, so scripts can branch on it.
`routedroid --help` lists them too.

| Code | Means |
|---|---|
| 0 | success; for `start`, the connection ended cleanly |
| 1 | `doctor` found a failing check |
| 2 | usage: a bad option, an unknown phone, a start the policy forbids, stopping what is not connected |
| 3 | the daemon is unreachable; start it with `systemctl --user start routedroid` |
| 4 | the daemon speaks another API version; restart it after an upgrade |
| 10 | adb: adb is missing, the phone is missing or unauthorized, or an adb command failed |
| 11 | transport: the phone is not on a transport Routedroid allows (USB only, by default) |
| 12 | protocol: the app spoke the protocol wrongly |
| 13 | auth: the app and the PC could not prove themselves to each other |
| 14 | vpn: the phone refused or failed to bring the VPN up |
| 15 | helper: the root helper refused or failed |
| 16 | timeout: the app never connected, or a teardown is still running |
| 70 | internal error |
| 130 | Ctrl-C twice: stopped waiting, and the daemon finishes the stop |

For `start` without `--detach`, the code is how the connection ended. For example, a phone
unplugged for longer than `--reconnect-wait` ends with 10, and says it was not back in time.
