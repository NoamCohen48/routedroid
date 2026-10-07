# Remembered phones

The daemon can remember a phone: a name for it, the options it connects with, and whether
to connect it whenever it is plugged in.

## Remember while starting

```sh
routedroid start R58M --name pixel --remember
```

From then on:

- `pixel` works wherever a serial does: `routedroid stop pixel`, `routedroid start pixel`.
- When the phone is plugged in and authorized, the daemon connects it by itself, with the
  options it was remembered with (here, the LAN it joined).

`--name` alone remembers the phone too. `--remember` alone remembers it without a name.

## The commands

```sh
routedroid phones                          # list them
routedroid remember pixel --lan-if eno1    # change what is remembered
routedroid remember R58M --name pixel --no-auto
routedroid forget pixel                    # drop it
```

`routedroid remember PHONE` takes the serial or the name, and changes only what you give it.
Its options are those of `start`: `--name`, `--lan-if`, `--phone-ip`, `--mtu`, `--dns`,
`--no-dns` and `--reconnect-wait`. `--no-auto` keeps the options without connecting on
plug-in.

## When it connects

Only the moment the phone becomes ready counts: plugged in (or attached when the daemon
starts) and authorized. So:

- A phone you disconnect with `stop` stays disconnected until it is plugged in again.
- A connection that fails is not retried over and over. Plug the phone in again, or run
  `routedroid start`.

The daemon logs each one (`journalctl --user -u routedroid`), and `routedroid events` shows
the connection as it goes.

## Where they are kept

In `~/.config/routedroid/phones.toml` (or under `$XDG_CONFIG_HOME`). Edit it with the
commands above rather than by hand, since the daemon holds its own copy while it runs.

## In the TUI

The connect form has a Name field and a Remember box (Space ticks it). `f` forgets the
selected phone. See [The TUI](tui.md).
