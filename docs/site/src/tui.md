# The TUI

```sh
routedroid-tui
```

The TUI shows the attached phones, the selected phone's connection and a log, and does what
the CLI does, interactively. It fits an 80x24 terminal.

A phone that goes away while connected stays on screen as `gone`, with its connection
`reconnecting` and its address held, and comes back on the same row.

## Keys

| Key | Does |
|---|---|
| ↑ / ↓ (or k / j) | select a phone |
| `s` | connect the selected phone: opens the connect form |
| `x` | disconnect it (asks first: `y` to confirm) |
| `f` | forget the selected phone, if it is remembered |
| `r` | refresh |
| PgUp / PgDn / End | scroll the log |
| `q`, Ctrl-C | quit |

Quitting leaves connections running: they belong to the daemon.

## The connect form

Tab and Shift-Tab move between fields, Enter connects, and Esc closes the form. The draft is
kept, so Esc or a failed start loses nothing.

| Field | Means |
|---|---|
| LAN interface | picked with ← / → from the interfaces the policy allows; empty: the one allowed |
| Phone IP | a chosen address; empty: leased by DHCP. The allowed blocks are shown beside it |
| DNS | servers separated by commas, or `none`; empty: the default |
| MTU, TUN, timeout, reconnect wait | as the `start` options of the same names |
| Name | a name for the phone; giving one remembers it |
| Remember | Space ticks it: connect the phone whenever it is plugged in |
| Network ADB | Space ticks it: allow a network adb serial |

A phone already remembered opens with its name and Remember box filled in. See
[Remembered phones](remembered-phones.md).

<!-- TODO: the throughput graph, once it lands. -->
