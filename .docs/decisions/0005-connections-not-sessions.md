# 0005 — A phone on the LAN is a *device connection*

Status: accepted (during Phase 2). Refines the vocabulary of [0003](0003-daemon-split.md);
those documents keep the words that were true when they were written.

## Problem

"Session" meant four different things: a phone being on the LAN (the daemon's unit of work),
the authenticated conversation with the app, the helper's journaled bundle of privileged
mutations, and the adb-side bootstrap. Reading the daemon, "session" and "connection" also
blurred together: a client connection and a phone's connectivity are unrelated things with
unrelated lifetimes, and the code used one word for both.

## Decision

- A **device connection** is one phone reachable on the LAN. The daemon owns it: it outlives
  the client that asked for it, and any client may start, stop or watch any of them. This is
  what `DeviceConnection` (the handle), `DeviceConnections` (the table) and `ConnectionRun`
  (its task) are.
- A **client connection** is one CLI or TUI process attached to the control socket
  (`ClientConnection`). It owns nothing and holds only a handle for `devices`, `status`,
  `start`, `stop` and `subscribe` — at first an `Api` façade, replaced in
  [0006](0006-components-not-a-daemon-object.md) by the components themselves. Client
  connections are deliberately *not* registered anywhere
  — nothing needs a list of them, and clients learn about shutdown from the event bus.
- **Session** survives only where a wire protocol uses it: the app handshake and relay
  (`routedroid-proto`, the Kotlin app, `routedroidd`'s `session` module) and the helper's
  journal. The adb-side bootstrap is now `AdbBridge`, not `DeviceSession`.
- The control API speaks the same words: `ConnectionInfo`, `ConnectionState`,
  `Event::Connection`, `Response::Status { connections }`, `DeviceInfo.connection`.
  `API_VERSION` is 2, so an old client fails the version check with a clear message instead
  of a parse error. The request verbs stay `start`/`stop` (and so do the CLI subcommands).

## Consequences

- One vocabulary from the wire down to the daemon's internals; "session" in daemon code now
  always means the protocol's session.
- The dispatch from `Request` to `Response` moved into `ClientConnection`: protocol
  translation belongs to the layer that speaks the protocol, and the daemon no longer
  mentions wire types.
- Clients and daemon must be upgraded together (they already ship together).
