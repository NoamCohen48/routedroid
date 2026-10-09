# 0006 — Components, not a daemon object

Status: accepted (during Phase 2). Supersedes the `Api` handle introduced by
[0005](0005-connections-not-sessions.md); the split of [0003](0003-daemon-split.md) is unchanged.

## Problem

`Daemon` held everything the process owns — adb, the device inventory, the table of live
device connections, the event bus — and every client request was answered by calling a method
on it. `ClientConnection` therefore had to hold a handle to `Daemon`, so the dependencies
formed a cycle: `Daemon` builds `Server`, `Server` owns `ClientConnection`, `ClientConnection`
calls back into `Daemon`.

An `Api` façade over `Arc<Daemon>` narrowed what a client connection could *see*, but the
value it held was still the whole daemon. The boundary was a convention, not a fact: nothing
stopped a later method on `Api` from reaching `stop_all()` or the poll loops, and the cycle
was still there.

The fix is to split the god object rather than to label it: extract the parts a client
connection needs into components of their own, and the cycle disappears by construction.

## Decision

Three components, each responsible for one thing and each holding only what that thing needs.
`Daemon` keeps nothing else; `Api` and `Core` are deleted.

- **`AttachedDevices`** — keeps the daemon's picture of adb current. Holds `Adb` and nothing
  else, owns the poll loop, and publishes snapshots to its own `watch<Vec<Device>>`
  (`current()`, `changes()`). It speaks adb's vocabulary only: serial, state, model. It does
  not know what a device connection is, and it never touches the event bus.
- **`DeviceConnections`** — the authority for the connections we created. Holds `Adb`, the
  helper socket, the `EventBus` and `AttachedDevices`; owns the table, `start`/`stop`, the
  traffic ticker, and the `Event::Connection`/`Event::Traffic` it publishes. It may read
  `AttachedDevices` (to refuse an offline or unauthorized phone before opening a helper
  session); the reverse edge does not exist.
- **`EventBus`** — the broadcast channel. A leaf.

`Daemon` becomes A1: it constructs the three, hands them to `Server`, and owns `stop_all()` at
shutdown. Nothing depends on it. `Server` is about the socket — bind 0600, check `SO_PEERCRED`,
accept, signals. `ClientConnection` receives the three handles as separate parameters (each a
cheap `Arc`-backed clone), not a bundle: the parameter list is the declaration of what a client
can reach.

Every arrow points down:

```
main → Daemon → { AttachedDevices, DeviceConnections, EventBus } → Adb
         │
         └→ Server → ClientConnection → the same three
```

### Why these three, and not some other cut

The cut follows the lifetimes, not the call sites. `AttachedDevices` mirrors state that
something else owns: adb decides what is plugged in, we only observe it, and nothing a client
does makes a device appear. `DeviceConnections` is the opposite — we create its entries, we
destroy them, and their lifetime is ours. The two sets deliberately do not coincide: a device
is usually attached with no connection, and a connection outlives its device when a phone is
unplugged mid-session (adb stops listing the serial while the helper is still undoing the TUN,
the nft table and the sysctl claims — dropping the entry then would leak host state). Two
lifetimes, two components.

That is also why the join between them lives in neither. `DeviceInfo.connection` is a *view*
that puts a row from the mirror next to a row from the registry, and views belong to the layer
that speaks the wire.

### Why a client connection needs handles at all

A client connection owns nothing it can answer from: no devices, no connections, no history.
Every verb on the wire is a question about, or a mutation of, state that lives for the whole
process and is shared with every other client — `stop` usually stops a connection some *other*
client started, since `routedroid start` exits and the TUI stops it later. Because the
`match request` lives in the connection (decision 0005), the connection must hold handles to
that state. The question was only ever how wide those handles are, and the answer is: exactly
as wide as the three components, with no fourth thing behind them.

`Request::Version` is the exception that shows the rule — it is answered from
`env!("CARGO_PKG_VERSION")` on the spot, because it needs nothing outside the connection.

### How each request is answered

| Request | What the connection does |
|---|---|
| `Version` | answers from its own constants; touches no handle |
| `Devices` | `devices.current()` + `connections.states()` + `device::unusable`, joined into `DeviceInfo` |
| `Status` | `connections.info()` |
| `Start` | `connections.start(request)` — one lock: refuse a duplicate serial, phone IP or TUN, refuse a phone adb reports unusable, pick a free `phoneN`, insert the handle, spawn the task |
| `Stop` | `connections.stop(serial)` — flip that connection's stop switch, wait for `Ended` without holding the table lock |
| `Subscribe` | `events.subscribe()`, plus `devices.changes()` for the device view |

Rust has `OnceLock` and DI crates; neither is used. A singleton would put the connection table
in process-wide state, make tests share one table, and hide the very edges this decision exists
to make visible. Constructor injection — passing the handle in — is the idiomatic form here,
and "exactly one instance" comes from `Daemon` constructing each once.

## Consequences

- There is no path from the socket layer to `Daemon`. A client connection cannot shut the
  daemon down or reach a poll loop, because it has never heard of the type that owns them.
- Wire types live only in `ClientConnection`. It builds `DeviceInfo` by joining
  `devices.current()`, `connections.states()` and the pure `device::unusable` policy — once for
  `Request::Devices`, once when `changes()` fires, to emit `Event::Devices` on its own socket.
  Nothing below it mentions `Request`, `Response` or `DeviceInfo`.
- `Request::Devices` no longer forks an adb process per request; it answers from the cached
  snapshot, so a client cannot make the daemon spawn adb as fast as it can write lines. The
  answer may be up to one poll interval stale.
- The device list and the device event are the same snapshot, so they can no longer disagree.
- Device changes go over a `watch`, which coalesces: a slow client sees the newest list instead
  of `Lagged`. `Event::Devices` therefore no longer travels on the broadcast bus.
- `AttachedDevices` reports adb state only. "Unusable because it is a network-ADB serial" is our
  policy, not adb's fact, and stays a pure function called where a verdict is needed.
- The wire protocol does not change; `API_VERSION` stays 2.
