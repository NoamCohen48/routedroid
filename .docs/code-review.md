# Routedroid code review

Reviewed 2026-09-23 at commit `f198e53`. Code only; docs were out of scope. 247 findings.

| Component | Critical | High | Medium | Low | Nit | Total |
|---|---:|---:|---:|---:|---:|---:|
| Daemon, CLI, TUI & protocol | 0 | 7 | 20 | 18 | 1 | 46 |
| Android app | 2 | 10 | 34 | 40 | 20 | 106 |
| Privileged helper & infra | 0 | 11 | 32 | 37 | 15 | 95 |
| **All** | 2 | 28 | 86 | 95 | 36 | **247** |

Finding IDs: `R-n` daemon/clients/protocol, `A-n.n` Android, `H-/J-/M-/P-/D-/T-/X-/N-/W-/S-/E-/F-n` helper and infrastructure.

## Verdict

Routedroid's *shape* is right: a pure protocol crate with golden fixtures shared by Rust and Kotlin, an unprivileged daemon with typed clients, and a root helper with write-ahead journaling and a systemd cleanup backstop. The code is readable, files are small, and the default `cargo clippy` run is clean. Default `cargo test` passes, 107 tests in all.

It is not close to production. The failures cluster in three places, and all three are the places a network tool is judged on:

1. **Failure paths do not converge.** When something breaks mid-flight, each of the three processes can end up stuck or leaking:
   - The Android packet pumps can deadlock on teardown and leave a default route into a dead tunnel, with Stop and revoke both no-ops (A-1.1).
   - A rotation during bootstrap kills an authenticated session (A-2.1).
   - The helper skips undo when an IPC send fails (H-4), and deletes the journal even when rollback failed (J-3).
   - One torn journal line blocks the helper for good (J-1).
   - The daemon's session loop ignores a dead helper channel (R-3).

   The happy path was tested; teardown under stress was not.

2. **The privilege and trust boundaries are softer than the design claims.**
   - Membership in the `routedroid` group is enough to make root proxy-ARP the LAN gateway's address (H-1, H-2).
   - Cleanup identifies kernel objects by reusable *names*, so it can delete another session's firewall (H-3).
   - `ip route replace` and `adb reverse` without `--no-rebind` both silently take over other owners' state (H-7, R-5).
   - The single-shot app listener, the global `LaunchGate` and the device port taken from an intent extra let any app on the phone block every start (R-4, A-2.2, A-2.3).
3. **Phase-0 code is still on the production path.**
   - The daemon talks to `phase0-helper`, a crate whose header says "Throwaway quality".
   - That binary ships a SIGKILL hook and a test client.
   - `phase0-tunnel` implements a dead protocol version.
   - The only DHCP implementation, which is the v1 primary mode, sits in a probe crate.

Beyond those, the protocol is stringly typed in both languages (hex and addresses as `String`, VPN_READY formatted differently from CONFIGURE_VPN). The Kotlin side's fixtures run against a different JSON library than the one on the phone (A-4.1). The data plane allocates and copies per packet on both ends and couples both directions in one task (R-1, R-2, A-1.11). There is no CI, no lint policy, and zero tests for the CLI, the IPC wire, the Android app module and the helper's session/recovery logic.

## Fix first

These block any real-LAN use, roughly in order:

| # | Finding | Why it blocks |
|---|---|---|
| 1 | A-1.1 Android teardown deadlock | Phone keeps a VPN default route into a dead tunnel; only force-stop recovers |
| 2 | H-1 / H-2 helper trusts caller's interface and address | Group membership = LAN address takeover as root |
| 3 | H-3 name-based recovery | Crash cleanup can remove a live session's firewall while forwarding stays on |
| 4 | H-4 / J-3 / J-7 undo not guaranteed | Leaked nft tables and sysctl claims with no journal record |
| 5 | J-1 / J-2 journal fragility | One torn line blocks every future helper start |
| 6 | A-2.1 bootstrap dies on configuration change | Rotating the phone during consent kills the session |
| 7 | R-4, A-2.2, A-2.3 bootstrap DoS | Any app on the phone can prevent every connection |
| 8 | H-7, R-5 silent takeover of routes and reverse mappings | Two owners, one resource, no error |
| 9 | R-1, R-3 daemon data-plane fairness and ignored helper failure | Uplink stalls under download; silent packet loss |
| 10 | A-4.1 one strict JSON codec on Android | Fixture tests don't cover the JSON that runs on the phone |

## Redesign proposals

Ordered so each step makes the next one cheaper.

1. **Rename and prune before anything else.**
   - Delete `phase0-tunnel` and the Phase-0 phone scripts.
   - Rename `phase0-helper` → `routedroid-helper` (crate, binary, unit, socket, state dir).
   - Promote `phase0-dhcp` → `routedroid-dhcp` as a library the helper uses.
   - Put fault injection behind a cargo feature, and move the test client out of the root binary.
2. **Make the helper own policy.** Add a root-owned `/etc/routedroid/helper.toml` with allowed interfaces. The helper leases the phone address itself (DHCP) or ARP-probes a manual one and refuses gateway and neighbour addresses. Every kernel object is tagged with the session id (nft comment, route `proto`, TUN `ifalias`), and a name cannot be reused while any journal mentions it. A `SessionGuard` whose drop path runs the journaled undo replaces `?`-skippable cleanup. Use netlink instead of `ip`/`nft` text parsing.
3. **Give `helper-ipc` a real wire.** Add a `Hello{version}` handshake, a `Datagram` enum with encode/decode (no hand-assembled `[kind][payload]` in three places), and typed error codes.
4. **Split the daemon's data plane from its control plane.**
   - Two independent pumps per connection. The `Machine` sees control frames only.
   - Pinned timers, a buffered reader, batched writes and pooled buffers.
   - Consider passing the authenticated TCP fd to the helper (SCM_RIGHTS) so packets cross userspace once. It is a trade-off, because the root process would then parse frames. Decide it deliberately.
5. **Validate at the edge, type everything inside.** `StartRequest` → `ConnectionSpec` with newtypes (`InterfaceName`, `TunName`, `Mtu`, `UnicastV4`), rejected synchronously. The daemon gets its own error enums, and the wire `Kind`/exit codes stay in the IPC/CLI layer. `Outcome` becomes an enum. `stop` becomes asynchronous on the wire.
6. **Protocol v1, retyped while nothing is shipped.**
   - `Ipv4Addr` and one `Prefix` shape everywhere; hex fields deserialize straight into `Nonce`/`Proof`.
   - Canonical route prefixes; 20-byte minimum packets.
   - A consent-timeout error code.
   - The device port moves into the shell-delivered bootstrap record.
   - Per-role state enums.
   - Kotlin `:protocol` becomes a plain JVM module with one strict JSON codec.
7. **Rebuild the Android app around one owner.** A process-level `DeviceLink` state machine exposes `StateFlow<LinkState>`. Activity and service become thin shells. The packet path is threads or a poll loop with a single idempotent close. Add edge-to-edge handling, string resources, and `SUPPORTS_ALWAYS_ON=false`.
8. **Add CI and tests where the bugs are.**
   - fmt, clippy with `[workspace.lints]`, `cargo deny`, Gradle lint and unit tests.
   - Daemon tests with fake adb and a fake helper behind traits.
   - Helper plan, journal and recovery unit tests.
   - `cargo fuzz` for frame and body parsing.
   - JVM tests for the Android pumps using socketpairs.
   - Golden JSON for the IPC wire.

## How this review was done

- **Host daemon, clients and protocol crate:** reviewed directly. Every file was read, `cargo clippy` was run (default and `-W clippy::pedantic`), and so was `cargo test --workspace`.
- **Android:** a second reviewer read every file under `android/` and cross-checked against the fixtures and the Rust crate. Nothing was built or run. Claims about platform behaviour are marked *unverified* in the findings.
- **Privileged helper, phase-0 crates, integration tests and fixture tooling:** a third reviewer read every file. Four findings were reproduced in an unprivileged user and network namespace; they are marked **[reproduced]**.
- **Docs:** out of scope at the owner's request. They were used only to learn intended behaviour.

Severity scale:

- **Critical:** loses the device or the network.
- **High:** wrong behaviour or a security hole in a normal scenario.
- **Medium:** wrong under plausible conditions, or a design flaw that will cost later.
- **Low:** localized defect or cleanup.
- **Nit:** style.

## Architecture verdicts by component

### Android app

The protocol module is decent: small files, fixture-driven, strict header validation before allocation. The app layer, however, is a Phase-0 probe that grew features. The flaws are structural, not cosmetic:

1. **Lifecycle ownership is wrong.** The bootstrap flow (authentication thread, handoff, consent) is owned by an Activity instance. The session's protocol state is partly held in a UI store. Global mutable `object`s (`PendingConnection`, `BootstrapStore`, `StatusStore`, `LaunchGate`) are the only link between components. That is why config changes kill sessions (2.1), a second session is lost (1.3), and REVOKED logic reads UI state (1.5).
   **Proposal:** one process-level `DeviceLink` (or `SessionController`) owned by the application. It is a small state machine: `Idle → Authenticating → AwaitingConsent → Configuring → Active → Stopping → Idle`.
   - It holds the record, the socket, the runner and a `StateFlow<LinkState>`.
   - BootstrapActivity becomes a thin view: it asks the controller to authenticate and only renders consent.
   - The VpnService becomes a thin shell that calls `controller.attach(vpnService)` and forwards `onRevoke`/`onDestroy`.
   - The UI observes `LinkState`, the notification observes it too, and nobody else writes it.

2. **The packet path should be threads, not coroutines.** Every pump does blocking I/O (`Os.poll`/`read`/`write`, blocking `SocketChannel`) on `Dispatchers.IO`, mixed with suspending channel operations that closing the socket cannot wake (1.1). Structured concurrency then was not used (1.2).
   **Proposal:** four dedicated threads with `ArrayBlockingQueue`s of pooled slots, or a single `Selector`/`poll` loop over the socket fd and the TUN fd with a non-blocking TUN. Teardown is one idempotent `close()` that closes both fds and a wake eventfd, then joins the threads with a timeout. It is simpler, deadlock-free, and testable on the JVM with socketpairs.

3. **One codec, one JSON, same bytes everywhere.** Make `:protocol` a JVM module with a strict JSON implementation and a single stream reader used in production (4.1, 4.6). Typed decode results (`Inet4Address`, validated MTU) replace string plumbing (3.9).

4. **Bootstrap trust inputs should all come from the shell channel.** Move `device_port` (and optionally an expiry) into the record (2.3), drop LaunchGate (2.2), make invalid launches invisible (2.5), and make the provider report accept/reject back to `content write` (2.12).

5. **Proposed component split** (each ≲150 lines):
   - `bootstrap/`: `BootstrapProvider`, `RecordVault` (TTL-wiping store), `BootstrapActivity` (view only), `HostAuthenticator`
   - `link/`: `DeviceLink` (state machine + StateFlow), `LinkState`, `SessionEnd`
   - `vpn/`: `RoutedroidVpnService` (shell), `VpnBuilderConfig` (typed config → Builder), `LinkNotification`
   - `transport/`: `PacketPath` (threads/poll loop), `SlotPool`, `FrameIo` (from `:protocol`), `Keepalive` (with a clock)
   - `ui/`: `MainActivity`, `StatusScreen` (renders `LinkState` with resources)

Top blockers before calling this production-worthy: 1.1, 2.1, 4.1, 1.3, 3.1, 7.1.

### Privileged helper

The helper is the most security-critical part of the project. Right now it is still the Phase 0 spike: its
own header says "Throwaway quality" (`phase0-helper/src/main.rs:1-2`), and routedroidd depends on it at
runtime through the `/run/routedroid/phase0-helper.sock` path and the `routedroid-phase0-helper@` unit. The
journal-plus-systemd shape is sound. Four things are not:

1. **The helper trusts the controller to choose `lan_if` and `phone_ip`.** Anyone in group `routedroid`
   can make root proxy-ARP any unrouted address on any interface, including the LAN gateway (see H-1 and
   H-2). The privilege boundary is currently "group membership = LAN takeover". The helper has to own the
   policy: a root-owned allowlist of interfaces, an ARP probe, and a refusal of neighbour and gateway
   addresses. Ideally the helper also owns DHCP (promote `phase0-dhcp` into it) and accepts only an address
   it leased itself.
2. **Kernel ownership is not exact.** Undo and recovery identify a TUN, route or nft table by *name*
   (`phoneN`, `routedroid_phoneN`, `dst/32 dev phoneN`). A crashed session's cleanup can therefore delete a
   live session's resources after the name is reused (H-3). Tag every resource with the session id: an nft
   table `comment`, a route `proto`/`realm` number from a reserved range plus an exact match, and a TUN
   `ifalias`. Refuse to reuse a name while any journal (live or orphaned) mentions it.
3. **Undo is not structurally guaranteed.** `Active` has no `Drop`, and `?` on IPC sends skips `stop()`
   (H-4). Several "exists?" probes also fail open, and a failure to probe is treated as "absent" (H-5), so
   the journal can be resolved while state still remains. Make the session an RAII guard whose drop path
   (or a `finally`-style wrapper in `connection::serve`) always runs the journaled undo. Make every probe
   return `Result<bool>`, and make "cannot tell" keep the entry unresolved.
4. **Shelling out to `ip`/`nft`, parsing their text output, blocking inside async code.** Replace this
   with netlink (`rtnetlink`/`neli` for links/addresses/routes, `nftnl` or `nft -j` for nft). At minimum:
   absolute binary paths, a cleared environment, timeouts, and `spawn_blocking`.

**Proposed redesign** (one crate, `host/routedroid-helper`, binary `routedroid-helper`, unit
`routedroid-helper@.service`, socket `/run/routedroid/helper.sock`):

```
routedroid-helper/src/
  main.rs            CLI: serve | check | cleanup   (the test client moves to integration-tests or a dev bin)
  policy.rs          root-owned /etc/routedroid/helper.toml: allowed interfaces, address rules
  plan.rs            Plan::build (pure validation + kernel facts injected via a trait → unit-testable)
  kernel/{link,route,nft,sysctl}.rs   netlink-backed, each op: apply / probe -> Result<bool> / undo, exact ownership tags
  journal/{mod,file,tests}.rs         versioned header, torn-tail tolerant, lock-before-read, create-via-rename
  claims/{mod,tests}.rs               Claims struct (dir injected, no global), GC of holders without a journal
  session/{mod,guard}.rs              SessionGuard: Drop runs journaled undo; no path can skip it
  relay.rs           packet loop only (no protocol parsing)
  fault.rs           crash hook behind #[cfg(feature = "fault-injection")]
```

`helper-ipc` should own the whole wire: a `Datagram` enum with encode and decode, a `Hello { version }`
handshake, an `ErrorCode` enum, the default socket path and the size constants. Neither side should
hand-assemble `[kind][payload]` again.

**phase0-tunnel: delete it.** It implements protocol version 0 (`rd-p0-auth`, `RDB0`), which the app no
longer speaks. Its adb path targets `dev.routedroid.phase0/.BootstrapActivity`, which no longer exists
(the app is now `dev.routedroid`; see `android/app/build.gradle.kts:12` and `android/README.md:14`). It
duplicates `routedroid-proto` (frame, auth, messages, state, ipv4) and the helper's TUN code. Delete it
together with `integration-tests/phase0/{netns-tunnel.sh,lan-proxyarp.sh,phone-bootstrap.sh,phone-multialias.sh,fake_android.py}`.
Those are Phase 0 evidence that is already recorded in decision 0001 and in git history.

**phase0-dhcp: promote it, do not keep it as a probe.** Version 1's primary mode depends on DHCP
acquisition (architecture §6), and no production code does DHCP. This crate is the only DHCP
implementation, and its parsing layer is decent. Move it to `host/routedroid-dhcp` (library + tests; split
the 600–700-line files), fix the findings in §5, and wire it into the helper so that the helper hands out
only addresses it leased itself.

**"phase0-" naming is wrong** for anything production uses. routedroidd depends on `helper-ipc` (crate)
and on the phase0-helper *binary/unit/socket* at runtime (`routedroidd/src/host_network/client.rs:14`).
Rename everything (see N-1).

## Findings: Daemon, CLI, TUI & protocol

### routedroidd · session

#### R-1 · High: `biased` select starves inbound frames under downlink load

`host/routedroidd/src/session/driver.rs:76`

The loop uses `biased;` with `from_helper.recv()` ahead of `idle` and `in_rx.recv()`. While the TUN has packets queued (any sustained download to the phone), every iteration picks the helper branch, so inbound frames (uplink packets, PONG, STOP, VPN_ERROR) are never polled. The reader task blocks on its full channel, the TCP window closes and phone→LAN traffic stalls — and because `on_rx` never runs, a long burst can even trip the keepalive (idle is only polled when helper is quiet, but the peer's liveness is judged on stale `last_rx`).

**Change:** Drop `biased` (tokio's default random fairness) or, better, stop multiplexing both directions through one task: see the data-plane redesign. At minimum put `in_rx` before `from_helper` and add a per-iteration budget.

#### R-2 · High: One task carries both packet directions and the control plane

`host/routedroidd/src/session/driver.rs:45`

Uplink injection (`to_helper.send().await` in `on_frame`) and downlink framing (`out_tx.send().await` in `on_helper_packet`) are awaited inside the same select loop. A slow helper socket blocks downlink; a slow TCP writer blocks uplink and control. Architecture §8.4 asks for independent bounded queues per direction; this is head-of-line coupling by construction.

**Change:** Split into two pumps (phone→helper, helper→phone) owned by the driver, with the `Machine` handling only control frames. The reader task can route `IP_PACKET` straight to `to_helper` once Active; the driver loop handles control, keepalive and stop only.

#### R-3 · High: Helper send failure is silently ignored; session keeps running

`host/routedroidd/src/session/handlers.rs:74`

In `on_frame`, when `to_helper.send()` or `out_tx.send()` fails, `delivered` is false, the loop `break`s — and the function falls through to `None`, so the session continues. With the helper channel closed, every uplink packet is dropped silently and still counted by `bump_from_phone` (which happens before the send).

**Change:** Return `Some(SessionEnd::HelperClosed)` / `Some(SessionEnd::Transport(..))` on a failed delivery, and bump counters only after a successful send.

#### R-9 · Medium: Secret travels inside a `Clone`-able config and is swapped out with `mem::replace`

`host/routedroidd/src/session/machine.rs:34`

`SessionConfig` derives `Clone` and holds `Secret`; `Machine::new` extracts it with `mem::replace(.., Secret::new([0;32]))`, leaving a zero secret in `cfg` for the machine's life. `AdbBridge::take_secret` panics if called twice. The secret's single-use rule is enforced by convention and `expect`, not by types.

**Change:** Keep the secret out of `SessionConfig`; pass it to `Machine::new(cfg, secret, nonce)` by value. Model the bridge as typestate: `AdbBridge::bootstrap(self) -> (Launched, Secret)` so the secret can only be moved once.

#### R-16 · Medium: Hot path allocates and re-arms timers per packet

`host/routedroidd/src/session/driver.rs:73`

Every loop iteration builds two fresh `Sleep`s (`sleep_until` for idle and phase), each packet is copied into a new `Vec` three times (helper recv `to_vec`, `read_frame` body `vec!`, send-side copy into `out`), frames are read without a `BufReader` (≥2 read syscalls per frame) and written one `write_all` per frame with no batching.

**Change:** Pin two `Sleep`s once and `reset()` them; wrap the read half in `BufReader`; batch writes (drain the channel into one buffer before `write_all`); use `bytes::BytesMut` pools or `sendmsg` with an iovec for the kind byte instead of copying.

#### R-17 · Medium: Consent timeout is reported as `protocol_error`

`host/routedroidd/src/session/handlers.rs:39`

When the user simply ignores the VPN dialog for 120 s, the phone is sent ERROR `protocol_error: no progress from Configuring` and the CLI reports a protocol fault (exit 12). That is not a protocol violation and the message is not actionable.

**Change:** Add a dedicated code (`consent_timeout`) and a `Kind::Vpn` outcome with a message like "VPN permission was not granted within 2 minutes".

#### R-18 · Low: Handshake deadline is per state, not per handshake

`host/routedroidd/src/session/timers.rs:11`

The doc says Connected → Configuring must complete in 15 s, but `PhaseTimer` resets on every state change, so a peer gets 15 s before HELLO and another 15 s before AUTH (the test `consent_deadline_applies_after_hello_reset_the_clock` enshrines this).

**Change:** Decide which is intended; if 15 s total, don't reset between Connected and Authenticating.

#### R-19 · Low: Writer task leaks on flush timeout

`host/routedroidd/src/session/driver.rs:104`

After 500 ms `finish` drops the `JoinHandle` without aborting, so a writer blocked on a stalled TCP peer keeps the socket and task alive until the kernel gives up.

**Change:** Abort the writer after the timeout.

### routedroidd · connection

#### R-4 · High: First connector wins: a hostile phone app (or any local user) can DoS every start

`host/routedroidd/src/app_listener.rs:34`

`accept()` consumes the listener after the first loopback connection. On the phone, `adb reverse` exposes `127.0.0.1:<device_port>` to every app, and the device port range is a fixed, tiny 17000–17999. A hostile app that loops connecting to that range grabs the single slot; the real app is refused, the host waits 15 s for HELLO, then fails the whole connection. On the host, any local user can do the same to the loopback port (visible in /proc/net/tcp). The `is_loopback` check is dead code — the socket is bound to 127.0.0.1.

**Change:** Keep accepting until one peer authenticates: run each accepted stream through HELLO/AUTH with its own short deadline, bounded concurrency (e.g. 2) and a total attempt budget; only the authenticated stream proceeds. Randomise the device port over a wider range. Delete the loopback check.

#### R-7 · Medium: Request validation happens after `Started` is returned

`host/routedroidd/src/daemon/connection/run.rs:58`

`mtu()` is checked inside the spawned task, so `start --mtu 70000` via the socket answers `Started` and then emits `Ended(usage)`. `lan_if`, `tun` (IFNAMSIZ, charset) and `phone_ip` (0.0.0.0, multicast, broadcast, loopback) are never validated by the daemon at all — only the root helper sees them. `Transport::check` runs twice (in `start` and again in `connect`).

**Change:** Parse the wire `StartRequest` into a validated `ConnectionSpec` (newtypes `InterfaceName`, `TunName`, `Mtu`, `UnicastV4`) in `DeviceConnections::start`, reject synchronously, and pass only the validated type to the task.

#### R-8 · Medium: The phone gets no DNS unless the operator remembers `--dns`

`host/routedroid/src/commands/start.rs:34`

`--dns` defaults to none and the TUI field is optional. With a 0.0.0.0/0 route and no DNS servers, name resolution on the phone silently breaks (Android may fall back to nothing inside the VPN). This is the most likely first-run failure.

**Change:** Default DNS to the LAN interface's DNS (helper can report it with `Started`) or at minimum the LAN gateway; make an empty list an explicit `--no-dns`.

### routedroidd · adb

#### R-5 · High: `adb reverse` silently rebinds another owner's mapping

`host/routedroidd/src/adb/reverse.rs:38`

`reverse_add` runs `adb reverse tcp:D tcp:H` without `--no-rebind`. Between `reverse --list` and `reverse_add` (two adb round-trips) another daemon, another user of the same adb server, or Android Studio can take the port; adb then *replaces* their mapping with ours. The collision retry the design promises does not exist — `reserve` tries exactly one port.

**Change:** Use `adb reverse --no-rebind`, and loop over candidate ports on the 'already exists' failure (bounded attempts).

#### R-30 · Low: `am_start` builds args with a parallel iterator trick and greps output for "Error"

`host/routedroidd/src/adb/android.rs:21`

Integer extras are stringified into a side `Vec` and consumed with `expect("one string per Int extra")`. Success is decided by `out.contains("Error")`, which misfires on localized or vendor output and misses warnings like "Activity not started, its current task has been brought to the front".

**Change:** Collect `Vec<String>` args directly. Use `am start -W` and parse `Status: ok`, or check for the explicit `Error type`/`Error:` prefixes.

### routedroidd · server

#### R-6 · High: A `stop` freezes that client's event stream for up to 30 s

`host/routedroidd/src/server/client.rs:60`

`run()` awaits `on_line` inside the select, and `answer(Stop)` awaits `stop_and_wait_on` (STOP_WAIT = 30 s). During that time the connection forwards no events and reads no requests, so the TUI — which sends stop on its only connection — sees no `stopping`/`ended` transitions until the stop returns, and a busy bus overflows into `Lagged`. The TUI works around it with try_send + "busy; try again".

**Change:** Make `stop` asynchronous on the wire: reply `Stopping{serial}` as soon as the switch is flipped and let `Event::Connection{Ended}` report the result. If a blocking variant is wanted, run request handling in a spawned task per request and correlate by id (the protocol already has ids).

#### R-23 · Medium: A bad request line kills the client connection without a reply

`host/routedroidd/src/server/client.rs:87`

Any JSON or schema error (including an unknown request type from a newer client) closes the socket; the client sees EOF and has to guess why.

**Change:** Reply `Response::Error{kind: usage}` (with the id when it can be extracted) and keep the connection; close only on framing errors (line too long).

#### R-24 · Medium: Socket setup has permission and startup races

`host/routedroidd/src/server/bind.rs:17`

`create_dir` then `set_permissions(0700)` leaves a umask-mode window; `bind` then `chmod 0600` likewise. `exists → connect → remove_file` lets two daemons started together both conclude the socket is stale and unlink each other. `current_uid()` stats `/proc/self` and `expect`s; `socket::uid()` falls back to uid 0 on error (`/run/user/0`).

**Change:** `DirBuilder::new().mode(0o700)`; set umask 0o077 before bind; take an `flock` on `control.lock` before touching the socket; use `rustix::process::getuid()` in both crates.

### routedroid-proto

#### R-10 · Medium: `Secret` derives `PartialEq` (non-constant-time) and `Clone`

`host/routedroid-proto/src/auth/mod.rs:44`

Deriving `PartialEq`/`Eq` on the secret gives every caller a timing-leaky `==`; `Clone` multiplies copies of a value the design says exists once. Only tests need either.

**Change:** Remove both derives; if tests need comparison, use `ct_eq` via a test-only helper.

#### R-11 · Medium: Hex fields are `String`, forcing `expect` at use sites

`host/routedroid-proto/src/messages/handshake.rs:13`

`client_nonce`, `host_nonce`, `host_proof`, `android_proof` are validated as 64 lowercase hex in `validate()`, then decoded again with `.expect("validated 64 hex")` in `machine.rs:110,132`. The invariant lives in a different module from the panic that depends on it.

**Change:** Deserialize directly into `Nonce`/`Proof` newtypes with a `serde(with = hex32)` adapter; `validate()` then disappears for those fields and so do the `expect`s.

#### R-12 · Medium: Addresses are stringly typed and encoded two different ways

`host/routedroid-proto/src/messages/vpn.rs:6`

`Prefix.address` is a `String`, `dns` is `Vec<String>`, and `VpnReady.addresses` is `Vec<String>` in `"a.b.c.d/n"` form while `ConfigureVpn` uses `{address,prefix}` objects. The host then compares VPN_READY by formatting strings (`machine.rs:149`). Routes with host bits set (10.0.0.5/24) pass validation, but Android's `VpnService.Builder.addRoute` throws `IllegalArgumentException("Bad address")` for them.

**Change:** Use `Ipv4Addr` for every address and one `Prefix{address: Ipv4Addr, prefix: u8}` shape in both messages (bump the protocol while nothing is shipped). Reject non-canonical route prefixes in `validate()`; compare VPN_READY as parsed values and reject extra addresses.

#### R-13 · Medium: One `State` enum with role-dependent meaning; host never enters `Negotiated`

`host/routedroid-proto/src/state/mod.rs:8`

The host machine goes Authenticating → Configuring; `Negotiated` exists only for Android, yet the host allowlist and `PhaseTimer::budget` both carry a `Negotiated` arm. There are also two identical `Role` enums (`auth::Role`, `state::Role`).

**Change:** Either give each role its own state enum (`HostState`, `AppState`) or make the host actually pass through `Negotiated`. Merge the two `Role` enums.

#### R-14 · Medium: Frame layer rejects a legal 20-byte IPv4 packet as a protocol violation

`host/routedroid-proto/src/frame/mod.rs:24`

`MIN_PACKET_BODY = 21` makes a header-only IPv4 packet (valid; e.g. protocol 59) a frame error that closes the session, while `ipv4::check` would accept it (≥20). Frame-level and packet-level rules disagree.

**Change:** Set the frame minimum to 20 and let `ipv4::check` own packet validity (drop-and-count, not close).

#### R-15 · Low: Stale doc comment references a nonexistent `Other` variant

`host/routedroid-proto/src/messages/error.rs:5`

"`Other` keeps unknown codes from a newer peer readable" — there is no `Other`; unknown codes are handled by `ErrorBody.code: String` + `code() -> Option`.

**Change:** Either add `Other(String)` and make `code` an `ErrorCode` via serde, or fix the comment.

### routedroidd · architecture

#### R-20 · Medium: The wire crate's `Fault` is the daemon's internal error type

`host/routedroid-ipc/src/fault.rs:62`

Decision 0006 says wire types live only in `ClientConnection`, yet `routedroid_ipc::fault::{Fault, Kind, Result}` is imported by `adb/`, `device/`, `host_network/`, `app_listener`, `daemon/`. CLI exit codes (`Kind::exit_code`) live in the wire crate too. Kinds are also misassigned: listener timeout → `Vpn`, stop timeout → `Internal`, accept error → `Internal`.

**Change:** Give the daemon its own `thiserror` error enum per layer (`AdbError`, `HelperError`, …), map to the wire `Kind` in `server/client/answer.rs`, and move exit codes into the CLI crate.

### routedroid-ipc

#### R-21 · Medium: `Outcome{ok, kind, message}` and `Response::Ok` allow invalid or ambiguous states

`host/routedroid-ipc/src/api.rs:100`

`ok: true` with a `kind`, or `ok: false` with no kind, are representable (the CLI maps the latter to exit 70). `Response::Ok` answers both `Subscribe` and `Stop`, so the TUI has to remember the serial itself (`serial_of_stop.unwrap_or_default()`). Daemon-side outcomes are `Result<&'static str>`.

**Change:** `enum Outcome { Clean{reason}, Failed{kind, message} }`; per-request responses (`Subscribed`, `Stopped{serial}`); a typed `EndReason` enum in the daemon instead of static strings.

#### R-22 · Medium: `#[serde(untagged)]` server messages; zero wire tests

`host/routedroid-ipc/src/wire.rs:16`

Responses and events are distinguished by trial deserialization (presence of `id`). Errors from a malformed line become "data did not match any variant". The crate has no tests at all, so the JSON shape a non-Rust client relies on is not pinned anywhere.

**Change:** Tag the envelope explicitly (`{"kind":"response","id":..}` / `{"kind":"event",..}`) and add golden JSON tests for every Request/Response/Event variant, like the protocol fixtures.

### routedroidd · daemon

#### R-25 · Low: `PollTask` duplicates `Background`

`host/routedroidd/src/daemon/devices.rs:29`

`devices.rs` hand-rolls an abort-on-drop `JoinHandle` wrapper identical to `daemon/background.rs`.

**Change:** Use `Background` (or `tokio_util::task::AbortOnDropHandle`) in both places.

#### R-26 · Low: Polls `adb devices -l` every 2 s forever

`host/routedroidd/src/daemon/devices.rs:45`

A process spawn every 2 s for the daemon's lifetime, plus up to 2 s latency on plug/unplug.

**Change:** Use adb's push API (`adb track-devices -l` or the `host:track-devices-l` service on the adb server socket) and fall back to polling only if it fails.

#### R-27 · Low: Traffic ticker publishes every second even when nothing changed

`host/routedroidd/src/daemon/connections/traffic.rs:13`

Every active connection emits `Event::Traffic` each second regardless of subscribers or change, and only packet counts exist — no bytes, drops (`bad_packets` is not in `Counters`), or rates.

**Change:** Publish only on change; add bytes and dropped counters to `Counters` and `ConnectionInfo`.

#### R-28 · Low: `DeviceConnection::spawn` is clone soup with mixed public fields and getters

`host/routedroidd/src/daemon/connection.rs:56`

Eight manual clones (`serial`, `handle_serial`, `task_tun`, `task_counters`…) and a struct with some `pub` fields and some getter-only fields. `ConnectionRun<'a>` borrows `&StateSink` purely to avoid moving it.

**Change:** Put the immutable identity (`serial`, `lan_if`, `phone_ip`, `tun`, `started_at`) in one `Arc<ConnectionSpec>` shared by handle and task; make all fields private; let the task own the sink.

#### R-29 · Low: `tokio::sync::Mutex` where no lock is held across `.await`

`host/routedroidd/src/daemon/connections.rs:24`

The module comment promises no one holds the lock across an await; a `std::sync::Mutex` (or `parking_lot`) is then cheaper and makes that promise compiler-checked for `Send` futures.

**Change:** Switch to `std::sync::Mutex`.

### routedroidd · device

#### R-31 · Low: `state_name` derives wire strings from `Debug` output

`host/routedroidd/src/device/usable.rs:19`

`format!("{state:?}").to_lowercase()` ties the wire value to Rust variant spelling.

**Change:** Explicit match, or `serde` rename on `DeviceState`.

### routedroidd · host_network

#### R-32 · Low: Comments describe a "LAN alias" the design forbids

`host/routedroidd/src/host_network/mod.rs:1`

`HostNetwork` and its module say it holds "the LAN alias for the phone"; the architecture explicitly never adds an alias (it is a /32 route + proxy ARP). The default socket path is still `phase0-helper.sock`.

**Change:** Fix the wording; rename the helper and its socket once it stops being phase-0 (see the helper section).

### routedroidd · main

#### R-33 · Low: Invalid `--log` filter silently becomes `info`

`host/routedroidd/src/logging.rs:17`

A typo in `RUST_LOG` is swallowed.

**Change:** Fail startup with the parse error.

#### R-34 · Low: User unit has no hardening and a hard-coded path

`host/routedroidd/systemd/routedroid.service:1`

No `NoNewPrivileges`, `ProtectSystem`, `PrivateTmp`, `RestrictAddressFamilies=AF_UNIX AF_INET`, `LockPersonality`, etc.; `ExecStart=%h/.local/bin/routedroidd` hard-codes an install location.

**Change:** Add the hardening directives that work in user units and install via a packaging step that rewrites the path.

### routedroid (CLI)

#### R-35 · High: API-version mismatch is reported as "daemon unreachable" (exit 3)

`host/routedroid/src/connect.rs:25`

`Client::connect` both connects and checks `API_VERSION`; `connect()` wraps every error as `DaemonUnreachable`, so an outdated daemon prints "cannot reach routedroidd… start it with systemctl" — exactly the wrong advice. The TUI does the same (`main.rs:43`). `routedroid version` also cannot print the CLI's own version when the daemon is down.

**Change:** Return a typed error from `Client::connect` (`Unreachable` vs `Incompatible{daemon, api}`), map them to different exit codes and hints, and make `version` print the local version before connecting.

#### R-36 · Medium: `--json` is global but ignored by most commands

`host/routedroid/src/cli.rs:31`

`start`, `stop`, `version`, `events` ignore it (events is always JSON). `devices` prints a table without a header while `status` has one. Exit-code help text duplicates `Kind::exit_code` by hand.

**Change:** Either honour `--json` everywhere or make it a per-command flag; add a header to `devices`; generate the exit-code help from `Kind`.

#### R-37 · Low: Duration parsing is hand-rolled and permissive

`host/routedroid/src/commands/start.rs:48`

`trim_end_matches('s')` accepts `90sss`, rejects `2m`. Defaults (90 s, MTU range, 576) are duplicated between CLI, daemon and proto.

**Change:** Use `humantime`; take defaults and ranges from one shared constant (the daemon should own them, the CLI should just omit).

#### R-38 · Low: Ctrl-C stop errors are discarded

`host/routedroid/src/commands/start.rs:119`

The spawned stopper `let _ =`s both the connect and the `Stop` result, so a refused stop leaves the user waiting with no message. The Ctrl-C handler is installed only after `Started`; a Ctrl-C during the start request kills the CLI while the daemon proceeds. The prompt still says "abandon the session".

**Change:** Report stop failures to stderr; install the handler before sending `Start`; use the device-connection vocabulary.

### routedroid-tui

#### R-39 · Medium: A failed connection's reason vanishes from the main view

`host/routedroid-tui/src/app/events.rs:42`

On `Ended`, the device row's `connection` is cleared and the details pane forgets the connection; the only trace of *why* is one red line in a log that scrolls away (no timestamps, no scroll-back).

**Change:** Keep a `last_outcome` per serial and show it in the row and details pane until the next start; timestamp log lines and make the log scrollable.

#### R-40 · Medium: Start form is too thin to be the primary UX

`host/routedroid-tui/src/form.rs:1`

The operator must type the interface name from memory (no interface list exists in the API), cannot set MTU/TUN/timeout/network-adb, cannot move the cursor within a field or paste, loses all values on Esc or after each start, and the caret is a literal `_` instead of the terminal cursor.

**Change:** Add `Request::Interfaces` in the daemon and a picker in the form; remember the last values per serial; use a proper line-edit widget (e.g. `tui-input`) and `frame.set_cursor_position`.

#### R-41 · Low: Reconnect refreshes devices and status twice

`host/routedroid-tui/src/client_task.rs:27`

After a reconnect, `serve()` refreshes both lists and `App::apply(Incoming::Connected)` also returns `[RefreshDevices, RefreshStatus]`.

**Change:** Refresh in one place (the app's follow-up commands).

#### R-42 · Low: Layout breaks on real data

`host/routedroid-tui/src/ui/devices.rs:33`

Serial column is fixed at 20 (mDNS serials are ~45 chars); the device pane grows without bound and pushes the log off-screen with many devices; long log lines are clipped (no wrap); the status bar reason and hints overlap on narrow terminals.

**Change:** Min/max constraints with truncation+ellipsis; cap the device pane with scrolling; `Paragraph::wrap`; drop hints when width is short.

#### R-43 · Low: State wording duplicated between CLI and TUI

`host/routedroid-tui/src/describe.rs:5`

`describe::connection_state` and `output::state_word` render the same enum differently ("ended: …" vs "failed (vpn): …").

**Change:** `impl Display for ConnectionState` (or a shared `describe` module in the ipc crate) used by both.

### Workspace & tooling

#### R-44 · Medium: No CI, no lint policy, no supply-chain checks

`host/Cargo.toml:1`

There is no CI config anywhere in the repo, no `[workspace.lints]` (no `unsafe_code`, `clippy::pedantic` subset, `missing_docs` policy), no `rust-toolchain.toml`, no `cargo-deny`/`cargo-audit`. Edition is 2021. `clippy -W pedantic` reports ~290 warnings (casts that truncate, missing `#[must_use]`, redundant `continue`s). A committed `__pycache__/*.pyc` shows `.gitignore` is incomplete.

**Change:** Add CI running fmt, clippy (with a curated pedantic subset as errors), tests for Rust and Gradle, and the namespace rigs where possible; add `[workspace.lints]`, pin the toolchain, move to edition 2024, add `cargo deny`.

#### R-45 · Medium: Test coverage has holes exactly where bugs are

`host/routedroid-ipc/src/lib.rs:1`

`routedroid` (CLI) and `routedroid-ipc` have 0 tests. `routedroidd` has 21 unit tests but nothing exercises `DeviceConnections` (start/stop races, id-guarded removal), `ClientConnection` (subscribe, lagged, bad lines) or the relay. No fuzzing of `read_frame`/`messages::parse`, although the plan calls for it.

**Change:** Add a fake `Adb` and fake helper behind traits and write daemon-level tests; add `cargo fuzz` targets for frame and body parsing; add golden tests for the ipc wire.

#### R-46 · Nit: `use_small_heuristics = "Max"` produces unreadably dense lines

`host/rustfmt.toml:1`

e.g. `drive.rs:66` packs a guarded match arm, a state change and a flag assignment on one 120-column line.

**Change:** Use the default heuristics with `max_width = 100`.

## Findings: Android app

### Packet pumps, teardown, concurrency

#### A-1.1 · Critical: teardown deadlocks when the socket writer dies with a full tx queue, which leaves the VPN up with no way out

`app/…/transport/Pumps.kt:59-79`, `TunReader.kt:26,36`, `Pumps.kt:85-95`

- **Failure:** the phone uploads heavily and the host stops reading, or USB is unplugged while the write is blocked. `socketWriter` blocks in `ch.write`. `txFilled` (256) fills up, and `TunReader` suspends in `filled.send(slot)` (or in `free.receive()` when `txFree` is empty). Keepalive death, `fail()`, or user Stop then calls `closeSocket()`. `socketWriter` throws, and because `running` is already false the exception is swallowed. It exits **without draining `txFilled` or returning slots**. `TunReader` stays suspended in `send`/`receive` for good and never re-checks `running`, so `txFilled.close()` never runs and `jobs.joinAll()` never returns. `SessionRunner.run()` never reaches `teardown()`, `vpnFd` never closes, and the service never calls `finish()`. The phone keeps a VPN default route into a dead tunnel. The Stop button and `onRevoke()` both land in `SessionRunner.stop()`, whose `stopped` CAS makes them no-ops. Only a force-stop recovers.
- The mirror case exists on the rx side. `SocketReader` suspends in `free.receive()`/`filled.send()` (`SocketReader.kt:44,54`) after `TunWriter` died from a short write. `closeSocket()` does not wake a coroutine suspended on a channel.
- **Change:** failure must cancel channel operations, not just flip a flag and close the socket. Run the pumps in a `coroutineScope {}` (see 1.2) and have `fail()` cancel that scope, or at least `close()`/`cancel()` all four channels in `fail()` and `stop()`. Add a JVM test that kills each pump while the queues are full and asserts that `run()` returns.

#### A-1.2 · High: pumps are launched on the service-wide `SupervisorJob` scope instead of as children of the session

`Pumps.kt:58-76`, `SessionRunner.kt:100`, `RoutedroidVpnService.kt:28`

`Pumps` is given the service `scope` and calls `scope.launch` five times. The coroutines are siblings of the session job, not children:
- cancelling the session job does not cancel the pumps;
- a pump failure does not cancel its siblings (SupervisorJob);
- structured concurrency cannot be used to fix 1.1.

`SessionRunner.withDeadline` and `sendLast` use the same injected scope.
**Change:** `suspend fun run() = coroutineScope { … launch … }`, with no scope parameter. Cancelling on first failure is then free.

#### A-1.3 · High: a new session START is silently dropped while the previous session is still running or tearing down, which orphans an authenticated socket

`RoutedroidVpnService.kt:46-73`

- **Failure:** the host restarts a device connection (stop, then start again quickly). The old job is still `isActive` for about 0.5–1 s (the `STOP_FLUSH_MS` flush plus the `TunReader` 500 ms poll).
- The second `ACTION_START` logs "session already running" and returns. The new `Handoff` stays in `PendingConnection` with nobody to take it.
- `BootstrapActivity` has already set `handedOff = true`, so nothing discards it. The host waits in Configuring until its own deadline.
- The old job then calls `finish()` → `stopSelf()` with no `startId`, which also kills anything the new START did.

**Change:** keep a queue or replace policy. Either stop the old runner and start the new one when its job completes, or reject explicitly: take the handoff and send `VPN_ERROR internal "busy"`. Use `stopSelf(startId)`.

#### A-1.4 · High: `SessionRunner.stop()` can write STOP or VPN_ERROR in the middle of the packet stream and still send IP_PACKETs after it

`SessionRunner.kt:136-152`, `Pumps.kt:42-43,85-95`

- `stop()` calls `pumps.stop()`, which only stops the readers. The tx writer keeps draining up to 256 queued frames.
- In parallel it launches `sendLast(STOP)`. `ChannelOutput`'s lock keeps frames whole but does not order them, so STOP can be followed by IP_PACKET frames. Spec §5 rule 7 says "a sender of STOP closes after sending", so the host sees traffic after STOP.
- **Change:** send the final frame through the tx pipeline. Close the tx queue, let the writer drain, then write STOP or VPN_ERROR as the last frame and close. Or drop the queue before writing STOP.

#### A-1.5 · High: the REVOKED branch is dead logic because `active` is always true

`SessionRunner.kt:140-146`

`stop()` sets `StatusStore.State.STOPPING` (line 140) before the launched block computes `active = state == ACTIVE || state == STOPPING`. So `active` is always true, and REVOKED always sends `VPN_ERROR`, even during Negotiated. The deeper flaw is that protocol decisions read the **UI status store**. **Change:** keep the protocol state inside `SessionRunner` (for example a `@Volatile var phase`) and never read it back from `StatusStore`.

#### A-1.6 · Medium: `onDestroy` → `stop(DESTROYED)` launches the socket close on a scope that is cancelled on the next line

`RoutedroidVpnService.kt:81-85`, `SessionRunner.kt:142-151`

The `scope.launch { … closeSocket() }` is usually cancelled before it is dispatched, so the socket is not closed. If the session is blocked in `Configure.await` (blocking I/O, not a suspension point), cancellation does not reach it either, and the thread stays blocked until the host sends something. **Change:** for DESTROYED, close the socket and the VPN fd synchronously inside `stop()`.

#### A-1.7 · Medium: the socket-reader verdict is wrong after a local stop

`SocketReader.kt:32,66`

When `running()` goes false the loop returns `End.HostClosed`, so a local stop is reported as "host closed". `SessionRunner` hides this with `localStop != null`, but when `fail()` set `running=false` and the failure is a `KeepaliveDead`, the verdict is still mislabelled. **Change:** return a distinct `End.LocalStop`, or `null`.

#### A-1.8 · Medium: the keepalive "Dead" verdict belongs to `SocketReader.End` but is produced by `Pumps`

`Pumps.kt:74,82`, `SocketReader.kt:26`

`End.Dead` is never returned by `SocketReader`. The keepalive lambda writes the shared local `end` from another coroutine, and `KeepaliveDead` exists only to be filtered back out on line 78. **Change:** give `Pumps` its own `sealed class PumpEnd`: host verdicts, `KeepaliveTimeout`, `LocalStop`, and `Failure(t)`, with no exception smuggling.

#### A-1.9 · Medium: slot pools are sized in slots, not bytes; host-chosen MTU 65 535 allocates about 34 MB of Java heap per session

`Pumps.kt:25-35`

2 × 258 slots × (mtu + 9) bytes. At the maximum legal MTU that is about 33.8 MB, allocated eagerly on the service thread, which risks OOM on low-RAM devices. **Change:** bound the pool by bytes (for example 1 MiB per direction, with `depth = max(8, budget / mtu)`), or allocate lazily.

#### A-1.10 · Medium: failures on the app side end the session without a VPN_ERROR

`SessionRunner.kt:62-66`, `Pumps.kt:45-49`

Short TUN writes, poll errors, keepalive death and `ErrnoException` all just close the socket. §4.6 says the app reports *everything* through VPN_ERROR (`internal`), so the host sees a bare EOF and cannot tell "phone failed" from "cable pulled". **Change:** send `VPN_ERROR internal` (bounded, via 1.4's path) for every non-host-initiated end except user Stop.

#### A-1.11 · Medium: the "no per-packet allocation" design is not what the code does

Hot-path allocations per packet:
- `ChannelInput.readHeader`: `ByteBuffer.wrap`, a `FrameHeader` data class, and a `Pair`;
- `readFully`: `ByteBuffer.wrap`;
- `ChannelOutput.write`: `ByteBuffer.wrap`;
- `Slot.setHeader`: `ByteBuffer.wrap`;
- `MessageType.fromCode`: an `entries` iterator.

That is roughly 7 allocations per packet per direction at 10k+ pps. **Change:** give each Slot one pre-wrapped `ByteBuffer`, keep a reusable header `ByteBuffer` in `ChannelInput`, use a 256-entry `Array<MessageType?>` lookup, and return the type and length through fields or a reusable holder.

#### A-1.12 · Low: a cross-thread `var timedOut` is not volatile

`HostHandshake.kt:77-85`, `SessionRunner.kt:117-125`

It is written by the watchdog thread or coroutine and read by the blocked thread after an exception. It is captured in a non-volatile `Ref.BooleanRef`. In practice the channel's internal locks probably publish it, but that is not guaranteed. **Change:** use `AtomicBoolean`.

#### A-1.13 · Low: PONG can queue behind 256 data frames or be dropped

`SocketReader.kt:56`

§5.1 asks for a PONG in under 1 s. Behind a full tx queue on a slow link it can be late. **Change:** use a priority control lane in the writer (check a `pendingPong` flag before each dequeue).

#### A-1.14 · Low: frames that are illegal while Configuring are accepted once Active

Between CONFIGURE_VPN and VPN_READY the app does not read the socket. A host PING or IP_PACKET sent during Configuring is accepted later by `SocketReader` as legal Active traffic. Only a peer bug triggers this, but the app is not strictly enforcing the §5 table. **Change:** acceptable if documented in code. Otherwise run a short Configuring-phase read before starting the pumps.

#### A-1.15 · Low: `TunWriter` catches `ClosedReceiveChannelException`, which a `for (x in channel)` loop never throws

`TunWriter.kt:24-26`. **Change:** delete the dead catch.



#### A-1.16 · Low: the `TunReader` poll timeout and keepalive tick cause 3 wakeups per second for the whole session

`TunReader.kt:18`, `Keepalive.kt:12`

**Change:** wake the reader with an eventfd or a self-pipe on stop, and compute the keepalive `delay` to the next deadline instead of ticking every second.

#### A-1.17 · Low (unverified): keepalive uses `elapsedRealtime`, which counts deep sleep

`Keepalive.kt:19`

If the SoC suspends with no USB traffic for 30 s or more, the first tick after wake declares the host dead even if it was pinging and simply could not wake the device. **Change:** confirm on hardware. Consider `uptimeMillis` for the dead check, or require two missed PING cycles after wake.

### Bootstrap, activity lifecycle, security

#### A-2.1 · Critical: any configuration change during bootstrap kills the authenticated session and strands a blank dialog

`BootstrapActivity.kt:61,89-97,141-144`; manifest has no `configChanges`

- **Failure:** rotating, toggling dark mode, a locale change, a fold/unfold, or entering multi-window while authentication runs or while the notification-permission or VPN-consent dialog is up. The old instance's `onDestroy()` runs with `handedOff == false` → `PendingConnection.discard()`, which closes the authenticated socket.
- The new instance hits `if (savedInstanceState != null) return` before setting any text, so it shows an empty dialog forever.
- If the consent result arrives in the new instance, `startTunnel()` starts the service with no handoff. The user sees "service started without an authenticated host connection" after granting consent.

**Change:**
- Move the flow out of the Activity into a process-level `BootstrapFlow` or a ViewModel that owns the thread, the handoff and the state.
- In `onDestroy`, discard only when `isFinishing && !isChangingConfigurations`.
- Use `registerForActivityResult`, which survives recreation.
- Render the current flow state in `onCreate` regardless of `savedInstanceState`.

#### A-2.2 · High: `LaunchGate` protects nothing and gives any app a way to DoS the real host launch

`BootstrapActivity.kt:62-67`, `LaunchGate.kt`

- The gate runs before the record check and is global. A hostile app that loops `startActivity(BootstrapActivity)` at 3 per 10 s makes every legitimate `am start` from the host fail with "rate limited".
- A launch without the matching random session (16 hex characters) already does nothing: no connect, no prompt. So the gate prevents no cost worth preventing.
- Its KDoc also claims "a record is consumed per attempt regardless", which is false (`BootstrapStore.take` keeps the record on mismatch).

**Change:** delete the gate, or apply it only to launches that matched a record (that is, handshake attempts). Test the DoS from the hostile app.

#### A-2.3 · High: the device port comes from an unauthenticated intent extra, so a caller who knows the session can point the app at its own listener and burn the record

`BootstrapActivity.kt:55,80`, `BootstrapRecord.kt`

The session id is not secret by design, and it is written to logcat (see 2.9). With it, a caller can launch with the right session and an attacker-chosen `device_port`. The app takes (consumes) the record, connects to the attacker's `127.0.0.1:port`, fails authentication, and wipes the record. The real host launch then finds nothing. **Change:** put `device_port` inside the shell-delivered bootstrap record (it is trusted), and verify that the intent extra matches it before calling `take()`. Alternatively, do not consume the record until authentication succeeds. This changes the record format, fixtures and Rust together.

#### A-2.4 · Medium: `singleTask` with no `onNewIntent` silently drops a second launch

`AndroidManifest.xml:34`, `BootstrapActivity.kt`

If the host retries (its 120 s consent deadline expired, then it starts again) while the previous BootstrapActivity is still up (consent dialog ignored), the new intent is delivered to `onNewIntent`, which is not overridden. The new record expires unused and the host fails again with no diagnostic. **Change:** handle `onNewIntent`: abort the old flow and start the new one. Or use `standard` launch mode with `finishOnTaskLaunch`/`noHistory`.

#### A-2.5 · Medium: an invalid launch is observable (spec §7.2 "MUST do nothing observable")

`BootstrapActivity.kt:56-60,62-75`

- With missing or invalid extras the activity stays open and shows a developer usage string (`bootstrap_missing_extras`) to whoever launched it, including any third-party app.
- The rate-limited and no-record paths set text and then `finish()` in the same frame. The window still flashes, and the text is never readable (dead strings).

**Change:** decide the verdict before `setContentView`. Call `finish()` without inflating UI (use a `Theme.NoDisplay`-style trampoline theme until the record matched), and log only.

#### A-2.6 · Medium: the app keeps no consent deadline, so a VPN is established against a host that already gave up

`BootstrapActivity.kt:117-139`, `SessionRunner.kt:85`; host `CONSENT_DEADLINE` = 120 s (`host/routedroidd/src/session/timers.rs:15`)

- **Failure:** the user answers the consent dialog after 120 s. The host has closed the connection, but CONFIGURE_VPN is already buffered.
- `Configure.await` reads it and `establish()` installs the default route. The VPN_READY write usually succeeds (peer FIN, not yet RST).
- The pumps start, `SocketReader` sees EOF, and everything tears down. The phone's networking drops briefly and the user sees "host closed the connection".

**Change:** record the auth time in the handoff and refuse to start when it is older than the host's consent deadline (share the constant through `:protocol`). Also probe the socket for EOF before `establish()`.

#### A-2.7 · Medium: the bootstrap record is not wiped at its 60 s TTL

`BootstrapStore.kt:21-46`

Expiry is checked only lazily in `take()`. If no launch follows, the secret stays in the heap until process death (hours). **Change:** schedule a wipe at `expiresAt` (for example `Handler.postAtTime`, or check on every `put`/`take` plus a timer).

#### A-2.8 · Medium: the "at most one record per 60 s" rule is not implemented

`BootstrapStore.kt:21-27`

`put()` replaces any pending record unconditionally. §7.1 says "MUST accept at most one record per 60 seconds". Only the shell can write, so the risk is low, but code and spec disagree. **Change:** either enforce it (and handle legitimate host retries), or get the spec changed. Do not leave them silently divergent.

#### A-2.9 · Medium: the session id is logged at INFO, and it is the capability for 2.3

`BootstrapStore.kt:26`, `HostHandshake.kt:66`, `BootstrapStore.kt` warn logs

Apps granted `READ_LOGS` (possible via `pm grant`, since it is a development permission) and bug reports see it. **Change:** log a short hash, or nothing, until 2.3 is fixed.

#### A-2.10 · Medium: copies of the secret survive the "wipe"

`protocol/…/auth/Auth.kt:42-48`, `HostHandshake.kt:59,63`

Each `Auth.proof` builds a `SecretKeySpec`, which clones the key, and a `Mac` with ipad/opad state. It is called twice (verify, then proof), and none of these copies are destroyed. `BootstrapRecord.wipe()` clears only the original array. §7.3 requires wiping "where the runtime permits". **Change:** compute both proofs from one `Mac` (`mac.init` once, then `doFinal`/`reset`), and call `(key as? Destroyable)?.destroy()` in try/catch. Document the residual copies inside the JCA provider.

#### A-2.11 · Medium: the provider's read timeout may not unblock the reader, and closing an fd from another thread risks fd-reuse reads (unverified)

`BootstrapProvider.kt:57-79`

On Linux, `close()` from another thread does not wake a thread blocked in `read()`. Whether `ParcelFileDescriptor.closeWithError` triggers libcore's AsynchronousCloseMonitor is unverified. If it does not, the reader thread blocks forever on a stalled writer, and after the close the fd number can be reused by another open. **Change:** set `SO_RCVTIMEO` on the read end with `Os.setsockoptTimeval`, or `Os.poll` with a deadline before each read. Drop the second "killer" thread.

#### A-2.12 · Medium: the host cannot tell whether the record was stored

`BootstrapProvider.kt:37-54`

`openFile` returns immediately and validation happens asynchronously, so `content write` exits 0 even for a malformed record. The failure only shows as an unexplained "no record" later. **Change:** after draining, report rejection with `readEnd.closeWithError(reason)` (reliable socket pair) so the writer side can see it. Verify on device that `content write` surfaces it (unverified), or have the host confirm through a DUMP-guarded `call()` that reports "record present for session X".

#### A-2.13 · Low: the write descriptor is a bidirectional socket, while §7.1 says "MUST return a write-only descriptor"

`BootstrapProvider.kt:50-53`

**Change:** `Os.shutdown(pair[1].fileDescriptor, SHUT_RD)` before returning it (or `SHUT_WR` on the read end).

#### A-2.14 · Low: the record's reserved bytes are not checked

`protocol/…/auth/BootstrapRecord.kt:31-41` (Rust has the same gap)

§7.1 says `reserved[3] = 0`. A record with non-zero reserved bytes is accepted, so a v1.1 layout change could be misread. **Change:** reject non-zero `bytes[5..8)` on both sides, and add an `invalid` fixture.

#### A-2.15 · Low: `BootstrapRecord` is a `data class` carrying a secret

`BootstrapRecord.kt:6`

`copy()` and `component2()` hand out the same mutable array, and `equals` compares secrets without constant time. **Change:** make it a plain class. Expose `session` and a `use(block)` accessor for the secret, and drop `equals` (tests can compare `encode()` output).

#### A-2.16 · Low: `ConsentDenied` runs a detached raw thread and sets UI status before it knows whether a handoff exists

`ConsentDenied.kt:18-30`

It also lives in the root package, beside the activity, rather than in `bootstrap/`. **Change:** fold it into the bootstrap flow object from 2.1.

#### A-2.17 · Low: the auth thread captures the Activity

`BootstrapActivity.kt:78-98`

A raw `thread {}` holds `this` for up to 15 s. **Change:** use the flow object or ViewModel from 2.1 (lifecycle-independent), with a coroutine on `Dispatchers.IO`.

#### A-2.18 · Low: the deprecated `startActivityForResult`/`onRequestPermissionsResult` are used with `@Suppress`

`BootstrapActivity.kt:101-139`

**Change:** use the Activity Result API. This is also part of the 2.1 fix.

### VpnService, configuration, foreground service

#### A-3.1 · High: no opt-out from always-on VPN

`AndroidManifest.xml:49-60`

The user can pick Routedroid as always-on VPN with "Block connections without VPN". The system then starts the service with no host. `onStartCommand` falls into `else -> finish()`, and lockdown blackholes all phone traffic until the setting is undone. **Change:** add `<meta-data android:name="android.net.VpnService.SUPPORTS_ALWAYS_ON" android:value="false"/>` to the service.

#### A-3.2 · Medium: the VPN is metered by default and inherits the phone's own network capabilities

`VpnConfigurator.kt:21-29`

- For targetSdk 29+ a VPN is metered unless `setMetered(false)` is called, so apps defer syncs and backups over the USB tunnel.
- With no `setUnderlyingNetworks`, Android reports the phone's Wi-Fi or cellular network as the VPN's underlying network, which is false: the traffic goes to the PC.

**Change:** call `setMetered(false)` (API 29+) and `setUnderlyingNetworks(emptyArray())`.

#### A-3.3 · Medium: `establish()` returning null is mapped to the wrong error code

`VpnConfigurator.kt:40` (and `:36` for `SecurityException`)

Spec §4.6: `vpn_establish_failed` means "`establish()` returned null or threw". The code sends `vpn_permission_denied` for null and for `SecurityException`, so the host tells the operator "user declined" when the user did not. **Change:** map both to `VPN_ESTABLISH_FAILED` and keep `vpn_permission_denied` for declined consent only. Also catch `IllegalArgumentException` from `establish()` itself (it currently falls through to `internal`).

#### A-3.4 · Medium: FGS type `specialUse` where `systemExempted` fits

`AndroidManifest.xml:6,52,57-59`, `VpnNotification.kt:37-41`

Android 14's FGS types document `systemExempted` as covering VPN apps configured through the VPN settings. `specialUse` requires a Play justification and a free-text subtype. **Change:** switch to `FOREGROUND_SERVICE_SYSTEM_EXEMPTED` / `systemExempted`, after verifying on API 34/35/36 that a consented VpnService qualifies (unverified). If it does not, keep `specialUse` and state that in the manifest comment.

#### A-3.5 · Medium: the notification has no Stop action and no state, and uses a Bluetooth icon

`VpnNotification.kt:25-36`

The icon is `android.R.drawable.stat_sys_data_bluetooth`, which is misleading. The text is static ("Raw IP tunnel…"): no address, no connected host, no error. There is no Stop action, so stopping means opening the app. **Change:** ship a vector small icon. Show the alias and state, and update the notification on state change. Add a Stop action (an explicit immutable PendingIntent to the service with `ACTION_STOP`), and `setCategory(CATEGORY_SERVICE)`.

#### A-3.6 · Medium: two different "state" enums, one of them held in a UI store

`session/StatusStore.kt:13`, `protocol/…/session/State.kt`

- `StatusStore.State` (IDLE, CONNECTING, NEGOTIATED, …) duplicates protocol `State` with different names. There is no AUTHENTICATING, CONNECTING really means authenticating, and IDLE is never re-entered.
- 1.5 shows the harm.

**Change:** `StatusStore` exposes an immutable `SessionView` derived from runner events. The protocol phase lives in the runner.

#### A-3.7 · Medium: `StatusStore.reset` from a new bootstrap clobbers a running session's status and counters

`BootstrapActivity.kt:76`, `StatusStore.kt:40-43`

A second launch (see 1.3) zeroes the counters and sets CONNECTING while the first session is still ACTIVE. The service then drops the second START, and the UI shows a wrong state for the rest of the session. **Change:** reset only when the runner actually starts a session, and key status by session.

#### A-3.8 · Low: `protect()` failure is fatal on a loopback socket that never needs it

`SessionRunner.kt:79`

Loopback never routes into the VPN (the local table wins), so a `false` from `protect()` on some vendor build would kill an otherwise working session. **Change:** keep the call (spec) but log a warning instead of failing when the peer is loopback. Or keep it fatal and document why.

#### A-3.9 · Low: CONFIGURE_VPN addresses are parsed twice, as strings

`VpnConfigurator.kt:44-45`, `protocol/…/message/Json.kt:63-68`

The protocol layer validates the text and `VpnConfigurator` splits it again with `toInt()`. The MTU equality rule sits in `VpnConfigurator.check` but is called from `Configure`. **Change:** have `ConfigureVpn.decode(body, negotiatedMtu)` return typed `Inet4Address`/prefix values and enforce MTU equality there ("parse, don't validate"). `VpnConfigurator` then only drives the Builder.

#### A-3.10 · Low: no semantic validation of the configuration

`ConfigureVpn.kt`

The address can be `0.0.0.0/32`, loopback, multicast or broadcast, and DNS `0.0.0.0` is accepted. Some are caught by the Builder as `IllegalArgumentException`, but not all (for example a multicast DNS). The host is trusted after auth, but §4.4 says to reject rather than pass bad values to the Builder. **Change:** reject non-unicast addresses and DNS, and prefix < 32 for the alias (decision 0001 adopts /32).

#### A-3.11 · Low: the service's session outcome is a `String?`

`SessionRunner.kt:53`, `RoutedroidVpnService.kt:63-71`

It cannot drive UI or localisation, and success and failure are told apart by null. **Change:** use a sealed `SessionEnd` (HostStop, HostError(code), LocalStop, Timeout, Violation, Failure) and render it in the UI layer.

#### A-3.12 · Low: `RoutedroidVpnService.finish()` shadows Activity vocabulary and calls `stopSelf()` without a `startId`

`RoutedroidVpnService.kt:87-90`

This is part of 1.3. **Change:** rename it to `stopService(startId)` and use `stopSelf(startId)`.

#### A-3.13 · Low: `VpnNotification.createChannel` takes a `Service` when it only needs a `Context`

`VpnNotification.kt:19`. It also recreates the channel on every service `onCreate`, which is harmless. **Nit-level.**



#### A-3.14 · Low (unverified): IPv6 bypasses the VPN

`VpnConfigurator.kt`

No IPv6 address or route is configured. Depending on the platform's default for an unconfigured family (see `Builder.allowFamily`), the phone's IPv6 traffic either leaks onto its own Wi-Fi or cellular network or is blocked. Either outcome is currently accidental. **Change:** decide explicitly. For "phone is a LAN host, IPv4 only", block IPv6 deterministically (add a ULA `/128` and `::/0`, and drop IPv6 in `TunReader`, which already happens).

### Protocol module and cross-implementation divergence

#### A-4.1 · High: the JVM tests do not exercise the JSON implementation that runs on the device

`protocol/build.gradle.kts:20-24`, `protocol/…/message/Json.kt`

Tests use Maven `org.json:20250517`; production uses Android's libcore fork of org.json. They differ in exactly the ways the fixtures are supposed to catch (per AOSP libcore source; confirm on device):
- **`JSONObject.quote` escapes `/` as `\/` on Android.** On a device, `VpnReady.encode()` therefore emits `{"addresses":["10.100.102.222\/32"],"mtu":1400}`. The fixture (and Maven) emit `…222/32…`. The "byte-exact against fixtures" tests pass on the JVM while the device emits different bytes. Serde accepts both, so it works by accident.
- Android's `JSONObject(String)` ignores **trailing garbage**, accepts **duplicate keys** (last one wins), **unquoted keys/values**, **single quotes**, and **hex (`0x…`) and octal (`017`) integers**. Serde rejects all of these, and architecture §8.3 says trailing bytes must close the session.
- On Android, `JSONArray.getString` coerces numbers to strings.

**Change:** make `:protocol` a plain Kotlin/JVM module (`kotlin("jvm")`, no Android plugin) with a JSON implementation that is the same bytes on device and in tests: `kotlinx-serialization-json`, or a small strict hand-written parser. Add fixture tests for trailing data, duplicate keys and non-string elements. Until then, run the fixture tests as instrumented tests on a device.

#### A-4.2 · Medium: length limits count UTF-16 code units, while Rust counts Unicode scalar values

`ConfigureVpn.kt:31` (`session_name`), `ErrorBody.kt:27` (`message`), `Hello.kt:31` (`app`), `VpnFailure.kt:8` (`take(512)`)

Rust uses `.chars().count()`. A host `session_name` of 40 emoji is valid in Rust and rejected by the app with `config_rejected`. A 300-character host ERROR message containing emoji becomes a "protocol: ERROR body" violation instead of the host's message. `take(512)` can split a surrogate pair. **Change:** use `codePointCount`, and truncate on code-point boundaries.

#### A-4.3 · Medium: the IPv4 validator accepts non-ASCII digits

`Json.kt:65` (`Char::isDigit`, `String.toInt()`), `VpnReady.kt:16` (`toIntOrNull`), `VpnConfigurator.kt:45`

Both accept Unicode decimal digits: `"١٠.0.0.1"` passes validation and becomes 10.0.0.1. Rust's `Ipv4Addr::from_str` rejects it. **Change:** use `it in '0'..'9'`, and parse into bytes once (see 3.9).

#### A-4.4 · Medium: non-UTF-8 bodies are silently repaired

`Json.kt:35`

`String(body, UTF_8)` replaces invalid bytes with U+FFFD, while serde rejects them. **Change:** decode with a `CharsetDecoder` set to `CodingErrorAction.REPORT`, and reject a BOM.

#### A-4.5 · Medium: `supported` elements and integers are coerced

`ErrorBody.kt:28`

`JSONArray.getInt` accepts `"1"` and `1.9`. Rust's `Vec<u8>` rejects both. **Change:** use `Fields.int`-style strict element checks.

#### A-4.6 · Medium: the tested frame reader is not the shipped one

`protocol/…/frame/FrameReader.kt` (tests only), `app/…/transport/ChannelInput.kt` (production, untested)

Two implementations of "read header, validate, then allocate". The fixture-tested one does not run on the device, and the one that runs has no tests. `Frame.decode` is also test-only. **Change:** move one reader into `:protocol`, abstracted over a `readFully(ByteBuffer)` source, used by the app and tested against `frames.json`. Delete the other.

#### A-4.7 · Low: host-side-only API is shipped in the app

`Hello.decode`, `AuthBody.decode`, `VpnReady.decode`, `HelloAck.encode`, `ConfigureVpn.encode`, `ErrorBody.protocolUnsupported`, `ErrorBody.knownCode`, `MessageType.fromName` (entirely unused), `Frame.packet` (unused), `Frame.decode`, `FrameReader`

They exist only for round-trip tests. **Change:** move them to test fixtures (`testFixtures` or the test source set), or accept them explicitly as "symmetric codec". Delete `fromName` and `Frame.packet`.

#### A-4.8 · Low: `Frame.decode` parses the header twice

`Frame.kt:30-31`. **Change:** parse once.



#### A-4.9 · Low: `Json.kt` mixes three concerns in one file

`BodyException`, the `JsonOut` writer and the `Fields` validators are in one file, and `Fields.mtu` uses fully qualified `dev.routedroid.protocol.Protocol` instead of an import (line 71). **Change:** split into `BodyException.kt`, `JsonWriter.kt` and `FieldRules.kt`, and import `Protocol`.



#### A-4.10 · Low: `JsonOut.key()` does not escape the key; `raw`/`objList` trust their callers

`Json.kt:17-28`

All keys are compile-time literals today, so this is latent. **Change:** quote keys with `JSONObject.quote`, or make `key` private-literal-only with a comment.

#### A-4.11 · Low: `Auth.transcript` validates neither `session` nor `devicePort`

`Auth.kt:30-40`

`devicePort` values of 65 536 and above wrap silently, and a non-ASCII session is accepted. Rust enforces `u16` by type. Its capacity hint (`96 + len`) is also wrong (99 + len). **Change:** `require(devicePort in 1..65535 && Protocol.validSession(session))`.

#### A-4.12 · Low: HELLO omits `app`

`HostHandshake.kt:52`

The host loses the app version for diagnostics and the "update the app" UX (§9). **Change:** send `"routedroid-android ${BuildConfig.VERSION_NAME}"`. This needs `buildFeatures.buildConfig = true` or `PackageManager`.

#### A-4.13 · Low: `readHelloAck` passes `DEFAULT_MTU` to a pre-negotiation read

`HostHandshake.kt:91`

The MTU is meaningless before HELLO_ACK. It only matters for IP_PACKET, which is rejected by the allowlist anyway. **Change:** give `readFrame` a control-only variant with no MTU parameter.

#### A-4.14 · Nit: `Hex.decode` is not constant-time

`Hex.kt:13-22`

It is applied to a public value (the received proof), so this is fine. A comment saying so would stop the next reviewer asking.

### UI, UX, accessibility, i18n, resources

#### A-5.1 · High: edge-to-edge is not handled at targetSdk 36

`res/layout/activity_main.xml`, `ui/MainActivity.kt`

On Android 15+ with targetSdk 35+, edge-to-edge is enforced, and at 36 the opt-out is ignored. The ScrollView has no insets handling, so the headline sits under the status bar and the Stop button can sit under the gesture/navigation bar. **Change:** call `enableEdgeToEdge()` and apply `WindowInsetsCompat.Type.systemBars()` padding to the root, or `android:fitsSystemWindows="true"` on the ScrollView.

#### A-5.2 · Medium: the main screen is a debug dump, not a UI

`ui/StatusText.kt`, `activity_main.xml`

- It shows raw enum names ("ACTIVE"), the session id and byte counters in monospace.
- Errors are one line at the bottom ("last error: io: Broken pipe").
- There is no explanation of why VPN permission is needed and no actionable guidance ("reconnect the cable, run `routedroid start`").

**Change:** show a state headline (Connected to PC / Not connected / Error), the alias, and an error card with an action. Move the counters into an expandable diagnostics section. Use Material 3 components.

#### A-5.3 · Medium: user-visible strings are hardcoded

`StatusText.kt:8-18` (all labels), the `StatusStore.setError(...)` literals (`BootstrapActivity.kt:83`, `ConsentDenied.kt:19`, `RoutedroidVpnService.kt:56,77`, `SessionRunner` failure strings), and the `StatusStore.State` names

None of these can be translated. **Change:** have errors carry a typed reason (3.11), resolved to `R.string` in the UI. Use a string resource with placeholders for each status row.

#### A-5.4 · Medium: the Stop button is always enabled and does work with no session

`MainActivity.kt:32-34`

With no session, tapping Stop creates the service, creates the channel and stops it again. There is no feedback. **Change:** bind `isEnabled` to `state == ACTIVE/NEGOTIATED/CONFIGURING`, and show a snackbar or the new state after stopping.

#### A-5.5 · Medium: the UI polls every 500 ms instead of observing the `StateFlow`

`MainActivity.kt:17-26`

`StatusStore.status` is a `StateFlow` but is polled, and the counters are polled too. **Change:** `lifecycleScope.launch { repeatOnLifecycle(STARTED) { combine(status, ticker(1s)) … } }`, which needs `lifecycle-runtime-ktx`. The counters can stay polled, at 1 s.

#### A-5.6 · Low: status changes are not announced to TalkBack

`activity_main.xml:25-32`

**Change:** `android:accessibilityLiveRegion="polite"` on the state headline (not on the counter block, which would spam).

#### A-5.7 · Low: the bootstrap dialog `TextView` is `wrap_content` with no minimum width

`activity_bootstrap.xml`

With a Dialog theme it renders as a narrow sliver and, per 2.1, as an empty box after recreation. Monospace is odd for user-facing prose. **Change:** `match_parent` width, a progress indicator for "Authenticating…", and normal typography.

#### A-5.8 · Low: there is no application icon

`AndroidManifest.xml:9-13`

No `android:icon`/`roundIcon`, so the launcher shows the default robot (lint `MissingApplicationIcon`). **Change:** add an adaptive icon, and a monochrome layer for themed icons.

#### A-5.9 · Low: the theme is AppCompat, not Material Components/Material 3

`res/values/themes.xml`, `activity_main.xml:16,23` (`TextAppearance.AppCompat.*`)

There is no colour scheme and no dynamic colour. **Change:** use `Theme.Material3.DayNight.NoActionBar` with `DynamicColors`, and M3 text appearances.

#### A-5.10 · Low: unused or dead strings

`status_idle` (overwritten by the first tick). `bootstrap_rate_limited`, `bootstrap_no_record` and `bootstrap_auth_failed` are shown for 0 frames before `finish()` (2.5). `bootstrap_missing_extras` is developer-facing. **Change:** remove them or make them reachable.



#### A-5.12 · Low (unverified): after bootstrap, MainActivity may land in a task excluded from recents

`AndroidManifest.xml:30-35`, `BootstrapActivity.kt:152`

`singleTask` + `excludeFromRecents` + the default affinity, then MainActivity started with `NEW_TASK` from that task. **Change:** give BootstrapActivity `android:taskAffinity=""` so it never shares the main task. Verify in recents.

#### A-5.11 · Nit: the manifest has neither `android:supportsRtl` nor `android:localeConfig`, and `usesCleartextTraffic="false"` does nothing for raw sockets

`AndroidManifest.xml:9-13`.



### Hostile test app (`hostile/`)

#### A-6.1 · Medium: probe 3 always reports PASS

`HostileActivity.kt:52-59`

It launches BootstrapActivity and prints PASS without checking anything ("check that no VPN prompt appeared"). **Change:** after about 3 s, assert `ConnectivityManager.getNetworkCapabilities(activeNetwork)?.hasTransport(TRANSPORT_VPN) != true` (visible to any app), and report FAIL otherwise.

#### A-6.2 · Medium: the attacks that matter are missing

- Burning the LaunchGate to DoS the real host (2.2).
- Launching with a leaked session and an attacker-controlled port to burn the record (2.3).
- Launching with no or garbage extras and asserting that no window appears (2.5).
- `startService(RoutedroidVpnService, ACTION_STOP)`, expecting `SecurityException`.
- `openFile` in `"r"`/`"rw"` modes.
- `ContentResolver.call()` on the provider: `call()` is **not** covered by the provider's `android:permission`. It is harmless today because the default returns null, but a probe guards regressions.

#### A-6.3 · Low: the error classification is wrong

`HostileActivity.kt:39-41,48-50`

In the `catch (e: Exception)` arms, `e is SecurityException` is always false because `SecurityException` was caught first. Any other denial (for example `FileNotFoundException` or `IllegalArgumentException` from package visibility) is reported as FAIL. **Change:** classify explicitly: `SecurityException` means PASS, a successful write means FAIL, and anything else means INCONCLUSIVE.

#### A-6.4 · Nit: stale naming

The label is "Phase0 Hostile" (`hostile/src/main/AndroidManifest.xml:9`), `versionName "0.0-phase0"` (`hostile/build.gradle.kts:13`), and the KDoc refers to "Phase 0 §3.4". The URI is duplicated as a literal instead of shared with `BootstrapRecord.PROVIDER_URI`.

### Build, Gradle, tests

#### A-7.1 · High: the `:app` module has zero tests

`app/src/test/java/dev/routedroid/` is empty.

The code with the bugs above (Pumps teardown, Keepalive, SessionRunner stop and revoke, BootstrapStore TTL, LaunchGate, PendingConnection, ConsentDenied, VpnConfigurator error mapping, StatusStore) has no coverage. Most of it is pure logic:
- `LaunchGate` already takes `now` and is still untested;
- `Keepalive` needs an injectable clock;
- `Pumps` can run over a socketpair plus a pipe fd on the JVM.

**Change:**
- Add JVM tests for all of the above, including the 1.1 deadlock regression.
- Add at least one instrumented test for the bootstrap flow across recreation (2.1) and for consent denial.
- Use Maestro for the UI flow (the implementation plan already calls for it).

#### A-7.2 · Medium: release builds are unshrunk, unsigned and unlinted

`app/build.gradle.kts:19-23`

There is no `isMinifyEnabled`/`isShrinkResources`, no signing config, no `lint { abortOnError = true; warningsAsErrors = true; checkReleaseBuilds = true }`, and no baseline. Several items above (missing icon, unused resources, deprecated APIs) are lint findings that nobody sees. **Change:** enable R8 with a keep file (little is needed), strict lint in CI, and reproducible-build settings (`android.enableR8.fullMode`, and pinned versions via a version catalog).

#### A-7.3 · Medium: `:protocol` is an Android library despite being "pure Kotlin"

`protocol/build.gradle.kts:4-18`

It needs the Android SDK to build and test, and its JSON comes from the platform (4.1). **Change:** use `kotlin("jvm")` with `java { toolchain 17 }`, and have `:app` depend on it normally.

#### A-7.4 · Low: dead test dependencies in `:app`

`app/build.gradle.kts:41-42`

JUnit and org.json are declared for a test source set that has no tests.

#### A-7.5 · Low: no version catalog; the root build only declares `com.android.application`

Dependency and plugin versions are string literals across three build files. The root `plugins {}` block declares only `com.android.application`, and `com.android.library` in `:protocol` resolves implicitly. **Change:** add `gradle/libs.versions.toml` and declare every plugin in the root with `apply false`.

#### A-7.6 · Low: `versionName = "0.1-phase1"` and `versionCode = 1` are hand-edited

`app/build.gradle.kts:15-16`

**Change:** derive both from one source shared with the host crate version, so HELLO's `app` field (4.12) and the host agree on what "update the app" means.

#### A-7.10 · Low (unverified): dependency currency

`core-ktx 1.16.0`, `appcompat 1.7.1`, `coroutines 1.10.2`, AGP 9.0.1 / Kotlin 2.2.10. Check them against the current stable releases before pinning. Add `lifecycle-runtime-ktx` (5.5) and `activity-ktx` (2.18).



#### A-7.7 · Nit: stale comment in the root `build.gradle.kts:1`

"Phase 0 throwaway probe. Single module" — there are three modules, and this is not phase 0. It is a code comment, not a doc.

#### A-7.8 · Nit: `compileOptions` and `kotlin { jvmToolchain(17) }` are repeated in three modules

**Change:** a convention plugin or `subprojects {}`/`allprojects` block.

#### A-7.9 · Nit: `:hostile` is built with the product

`settings.gradle.kts:18`

`./gradlew assemble` builds and signs a hostile app next to the real one. **Change:** keep it, but put it under `testing/` or behind a Gradle property, and never build it for release.

### Naming, comments, file split, idioms

#### A-8.1 · Low: files over the project's ~150-line rule

`SessionRunner.kt` (165) mixes the configure phase, the pumps phase, the stop policy and the final-frame policy; split it into `ConfigurePhase`, `ActivePhase` and `StopPolicy`. `BootstrapActivity.kt` (155) is UI plus flow plus permissions; 2.1 splits it naturally.

#### A-8.2 · Low: the package layout is inconsistent

`BootstrapActivity` and `ConsentDenied` sit in the root package, while the other UI is in `ui/` and the rest of the bootstrap is in `bootstrap/`. The `am start` component name is a wire contract (spec §7.2, `host/routedroidd/src/device/bridge.rs:14`), so moving the activity needs a coordinated host change; do it now, while it is free.

#### A-8.3 · Low: the app package `session/` holds `PendingConnection` (a bootstrap-to-service handoff) and `StatusStore` (UI state)

Neither is a "session" in the decision-0005 sense. Rename them to `handoff/` and `status/`.

#### A-8.4 · Nit: `Slot.PING`/`PONG` are shared singletons with a public mutable `len` and `buf`

A stray `setHeader` on them corrupts every future PING. Make `len` `private set` and use a separate immutable `ConstantFrame` type in the tx queue (`sealed interface TxItem`).

#### A-8.5 · Nit: `Pumps` has two readers of `running` (`@Volatile var`) plus `failLock`

An `AtomicReference<Throwable?>` and `compareAndSet` replace both.

#### A-8.6 · Nit: `HostHandshake.Failure`, `VpnFailure`, `FrameException`, `BodyException` and `Pumps.KeepaliveDead` are five exception types with no common base, and `SessionRunner` catches them by listing each

Give them a sealed `RoutedroidException`, or use result types.

#### A-8.7 · Nit: `Configure` is an `object` with one method and a nested `Outcome`, and `VpnConfigurator.check` is called from it (3.9)

Fold `check` into decode and rename `Configure` to `AwaitConfiguration` or `ConfigurePhase`.

#### A-8.8 · Nit: duplicated constants

`BootstrapActivity.EXTRA_SESSION` and `RoutedroidVpnService.EXTRA_SESSION` are the same literal defined twice, and the provider authority `"dev.routedroid.bootstrap"` appears in the manifest, `BootstrapProvider.AUTHORITY`, `BootstrapRecord.PROVIDER_URI` and the hostile app. Use a manifest placeholder, or at least one Kotlin constant.

#### A-8.9 · Nit: `BootstrapStore.clear()` is unused



#### A-8.10 · Nit: `BootstrapRecord.decode` uses `field.drop(end).any{}`, which boxes into a `List<Byte>`

Use a range loop.

#### A-8.11 · Nit: `ChannelOutput.write` is public and takes raw buffers, which lets callers bypass framing

Make it `internal`, or give it a `Slot` overload.

#### A-8.12 · Nit: `Keepalive.run(ping: () -> Boolean, …)` ignores the returned Boolean



#### A-8.13 · Nit: the `TunReader` comment "Oversize (interface MTU should prevent it)" is the only defence against a host/Android MTU mismatch

Count such drops separately from the non-IPv4 drops so the diagnostics can tell them apart.

#### A-8.14 · Nit: `StatusStore.setMtu` and `setConfig` both set the MTU, so `setMtu` is redundant



#### A-8.15 · Nit: in `VpnFailure.body()`, `message ?: code.wire`: `message` is never null because the constructor requires it



#### A-8.16 · Nit: the `Log.w(TAG, "host authentication failed: $e")` path writes the exception text

It is not secret today, but keep secret-adjacent paths terse.

#### A-8.17 · Nit: `ProtocolSession` naming

`SocketReader.End.HostStop`/`HostClosed` versus `Configure.Outcome.HostStop` are parallel types for the same concept in two phases. Unify them into one `HostEnd` type.

## Findings: Privileged helper & infra

### Security / privilege boundary (phase0-helper + units)

#### H-1 · High: any `routedroid` group member can make root proxy-ARP any unrouted address, including the gateway

`phase0-helper/src/session.rs:47-75`, `ops.rs:86-89`

`Plan::build` rejects only `host_ip`, 255.255.255.255, 0.0.0.0, multicast, and addresses that already have
a *main-table* `/32` route. Scenario: a user in group `routedroid` (the unit sets no `--allow-uid`, so the
socket's group is the only gate) sends `Start{lan_if:"eno1", phone_ip:<gateway IP>, tun:"phone9"}`. The
helper installs `gw/32 dev phone9` and enables `proxy_arp` on eno1. The host then answers ARP on the LAN
for the gateway, and other LAN hosts' traffic is hijacked or black-holed. The same works for any other
LAN host's address.
**Change:** before any mutation, (a) refuse the gateway and every address in the neighbour table of
`lan_if`, (b) run an RFC 5227 ARP probe (architecture §6.3, which the helper does not implement), and
(c) long term, accept only an address the helper leased via DHCP itself, plus an explicit root-configured
manual-address allowlist.

#### H-2 · High: no policy on which interface may be used

`session.rs:48-59`

Any existing non-`lo` interface is accepted as `lan_if`: `docker0`, `wg0`, `tailscale0`, a management NIC.
The helper then enables `forwarding` and `proxy_arp` on it. Architecture §2 and §4.2 require
operator-selected interfaces. The operator's choice currently lives only in the unprivileged daemon,
which any group member can bypass by talking to the socket directly.
**Change:** read a root-owned policy file (`/etc/routedroid/helper.toml`: `allowed_interfaces = [...]`) in
the helper and refuse anything else.

#### H-3 · High: recovery and undo delete resources by *name*, so a crashed session's cleanup can destroy a live session's firewall

`ops.rs:176-203` (undo), `ops.rs:207-214` (present), `recovery.rs:37-60`, `session.rs:57-65`

Scenario (Accept=yes, two instances): instance A (phone0) is SIGKILLed; its TUN and route vanish at once.
Before A's `ExecStopPost` runs, instance B (already running, waiting for Start, so its `ExecStartPre`
check has already passed) receives `Start{tun:"phone0"}`. `link_exists("phone0")` is false, so B creates
phone0, table `routedroid_phone0` and the route. Then A's cleanup runs: `present(Tun)` is true, so it
runs `ip link del phone0` (B's live TUN). `present(NftTable)` is true, so it runs
`nft delete table inet routedroid_phone0`, removing **B's deny rules while the forwarding claims are
held**. In `serve --socket` mode the same race happens inside one process between `Active::stop`'s
`drop(tun)` and its later `undo(Tun)`. Architecture §16 ("A session TUN name cannot be reused while any
unresolved ownership record remains") is not enforced by `Plan::build`.
**Change:** (1) under a global lock (the claims lock would do), refuse Start if any journal, live or
orphaned, mentions the same tun, table or phone address. (2) Tag resources with the session id (nft table
`comment "rd:<session>"`, route `proto <reserved>` plus exact `dst/dev/src` match, `ip link set phoneN
alias rd:<session>`) and have `present`/`undo` match the tag, not just the name.

#### H-4 · High: `?` on IPC sends inside the relay skips undo entirely (leaks nft table and sysctl claims)

`connection.rs:62-73` (`send_reply(...Started...).await?`), `connection.rs:102-103`

(`send_reply(...).await?` inside `select!`)
`Active` has no `Drop`. Any `?` after `session::start` succeeds returns from `serve` without
`active.stop()`. The TUN fd closes (the TUN and route vanish), but the nft table stays, the sysctl claims
stay held, and the journal stays unresolved. Realistic trigger: the daemon is killed while the helper is
applying the plan, so sending `Started` fails with EPIPE. Another trigger: the controller sends `Ping`
and exits. Under `Accept=yes`, `ExecStopPost` saves the situation. Under `serve --socket` (used by the
phase1 and phase2 rigs), the state leaks until the next process start, and that start then *refuses to
run* (`serve.rs:25`).
**Change:** make the active session an RAII guard, or restructure so that the relay returns an enum
(`Ended::Stop | Ended::Disconnect | Ended::Error`) and `stop()` runs unconditionally afterwards. Never use
`?` between start and stop.

#### H-5 · High: "exists?" probes fail open, so the journal is closed while kernel state remains

`ops.rs:77-84` (`link_exists` → `unwrap_or(false)`), `ops.rs:87-93` (`host_route_exists`,

`route_exists` → `unwrap_or(false)`), `ops.rs:104-106` (`nft_table_exists` → `unwrap_or(false)`),
`claims.rs:114-119` (`holds` swallows read and parse errors)
In `recovery::cleanup`, `Phase::Pending` plus `present()==Ok(false)` leads to `journal.undone(...)`, and
`undo()` treats "cannot tell" as "absent → Ok". If `nft` fails transiently (ENOBUFS, EPERM in a
misconfigured sandbox, a missing binary), cleanup records `Undone` and resolves the journal. The table
stays in the kernel forever with no record. `host_route_exists` failing open in `Plan::build` lets a
duplicate address through. The `Err` arm in `recovery.rs:50-53` is dead code, because `present` never
errors.
**Change:** return `Result<bool>` and distinguish "not found" (exit status/ENOENT) from "error". An
error keeps the entry unresolved.

#### H-6 · High: `sysctl_path` maps VLAN names to a non-existent path, so every dotted interface fails **[reproduced]**

`ops.rs:35-42`, test `ops.rs:231-234`

For `net.ipv4.conf.eth0.100.forwarding` the code produces `/proc/sys/net/ipv4/conf/eth0/100/forwarding`.
The real path is `/proc/sys/net/ipv4/conf/eth0.100/forwarding`: a literal dot in the directory. Only the
*sysctl(8) key syntax* swaps `/` and `.`. Reproduced: a veth named `a.b` in a userns produced
`/proc/sys/net/ipv4/conf/a.b`, and `conf/a/b` does not exist. Architecture §6.1 explicitly supports VLAN
netdevices. `sysctl_read` fails, and Start fails for every `eth0.100`. The unit test pins the wrong
behaviour.
**Change:** keep the ifname literal (`format!("/proc/sys/net/ipv4/conf/{ifname}/{leaf}")`) and fix the
test. Better: pass `(ifname, leaf)` as typed values instead of re-parsing a dotted string.

#### H-7 · High: `ip route replace` silently steals an existing route **[reproduced]**

`ops.rs:149-152`

Reproduced: with `10.9.0.7/32 dev d0` present, `ip -4 route replace 10.9.0.7/32 dev d1` succeeds and the
route now points at d1. Combined with the TOCTOU between `host_route_exists` (checked in `Plan::build`)
and the apply, two concurrent sessions for the same address both succeed and the second steals the
first's traffic. Any route installed after the check by something else (NetworkManager, a VPN) is also
overwritten. Undo then deletes it.
**Change:** use `ip route add` (fails with EEXIST), and treat EEXIST as a refusal, not something to
undo. Serialize address allocation under the global lock (H-3).

#### H-8 · Medium: `nft -f` with `table …{}` merges into a pre-existing table **[reproduced]**

`ops.rs:110-141`, `ops.rs:153-171`

Loading the same `table inet t { chain c {...} }` twice succeeds and doubles the rules (reproduced: the
counter rule appeared twice). If a leftover `routedroid_phoneN` exists (see H-3/H-4), Start silently
merges into it: stale rules for an old phone address stay live, and `apply` reports success.
`Plan::build` never checks whether the table exists.
**Change:** emit `create table inet routedroid_phoneN` (fails if it exists) as the first line of the
script, or check with `Result<bool>` and refuse.

#### H-9 · Medium: `host_route_exists` misses the host's own secondary addresses and non-main tables **[reproduced]**

`ops.rs:86-89`, `session.rs:63-73`

Only `phone_ip == host_ip` (the *first* address) is rejected. Reproduced: a secondary address
`10.9.0.3/24` on the interface gives 0 lines from `ip -4 route show 10.9.0.3/32` (local routes live in
table `local`). A phone could be assigned the host's own secondary address, and addresses of other
interfaces or policy tables are not considered either.
**Change:** reject any address the host owns (`ip -4 addr` on all links, or netlink `RTM_GETADDR`) and
check `ip route get <ip>` / all tables.

#### H-10 · Medium: the directed broadcast and network addresses are accepted as phone addresses

`session.rs:71`

`is_broadcast()` is only 255.255.255.255. On `/24`, `phone_ip = x.y.z.255` or `x.y.z.0` passes.
**Change:** reject the network and broadcast addresses of `host_ip/lan_prefix` (except /31 and /32
semantics), plus link-local and loopback.

#### H-11 · Medium: the production binary ships a SIGKILL fault-injection hook read from `/run/routedroid/crash-at`

`session.rs:16-33`, `main.rs:25,35-37`, used at every stage including `cleanup` (`recovery.rs`)

Every apply and undo step does a `read_to_string` of a root-owned path, and the binary can be told to
kill itself at any journal boundary. The file is root-only and so is not an escalation, but a
production root daemon should not contain a self-destruct test hook. `--crash-file` is also a global
option on the production CLI.
**Change:** gate it behind `#[cfg(feature = "fault-injection")]`, enabled only by the test rigs' builds.

#### H-12 · Medium: the systemd hardening is incomplete, and one directive is a no-op

`systemd/routedroid-phase0-helper@.service:24-36`

- `ReadWritePaths=/proc/sys/net/ipv4` does nothing without `ProtectKernelTunables=yes`, because /proc/sys
  is already writable. Add `ProtectKernelTunables=yes` and keep only `/proc/sys/net/ipv4/conf` writable.
  Whether `ReadWritePaths` re-opens a subpath under `ProtectKernelTunables` is **(unverified)**; test it.
- Missing: `SystemCallFilter=@system-service`, `SystemCallArchitectures=native`, `LockPersonality=yes`,
  `MemoryDenyWriteExecute=yes`, `RestrictNamespaces=yes`, `RestrictRealtime=yes`,
  `RestrictSUIDSGID=yes`, `ProtectClock=yes`, `ProtectHostname=yes`, `ProtectKernelLogs=yes`,
  `PrivateIPC=yes`, `UMask=0077`, `ProtectProc=invisible`, `IPAddressDeny=any` (the helper never
  talks IP).
- `CAP_NET_RAW` is granted but unused (no AF_PACKET code in the helper), and `AF_INET` is allowed but
  unneeded by netlink-based code.
- It runs as uid 0. Net sysctls, TUN, routes and nft all need only `CAP_NET_ADMIN`: `net_ctl_permissions`
  grants owner bits to CAP_NET_ADMIN holders. Run as `User=routedroid-helper` with
  `AmbientCapabilities=CAP_NET_ADMIN`. That the whole op set works unprivileged this way is
  **(unverified)**; try it.
- `KillMode=process`: in-flight `ip`/`nft` children survive a stop. Use the default `control-group`.

#### H-13 · Medium: no bounds on pre-Start behaviour, so any group member can pin up to 16 root processes indefinitely

`connection.rs:27-53`, `systemd/routedroid-phase0-helper.socket:12`

There is no deadline for `Start` and no limit on invalid requests. Each bad `Start` spawns 3–5 `ip`
processes (`Plan::build`). With `Accept=yes` and `MaxConnections=16`, one user can hold every helper slot
forever and block all phones.
**Change:** add a Start deadline (e.g. 10 s), close after the first invalid request, and set
`MaxConnectionsPerSource=`.

#### H-14 · Medium: fd 3 from systemd is not marked CLOEXEC, so every `ip`/`nft` child inherits the controller socket

`helper-ipc/src/seqpacket.rs:95-106`

systemd passes activation fds without `FD_CLOEXEC`. `sd_listen_fds()` sets it and unsets
`LISTEN_*`; this code does neither. Every child `ip`/`nft` holds the controller connection, so a helper
SIGKILLed mid-`nft` does not immediately close the controller's socket, and children inherit
`LISTEN_PID`/`LISTEN_FDS`.
**Change:** `fcntl(F_SETFD, FD_CLOEXEC)` (`rustix::io::fcntl_setfd`) and `remove_var("LISTEN_*")`. A
malformed `LISTEN_PID` should be an error, not a silent "not activated" (`seqpacket.rs:88`).

#### H-15 · Medium: `Tun::create` attaches to an existing TUN of the same name

`tun.rs:24-58` ("Create (or attach to)")

If a persistent `phoneN` TUN appears between `Plan::build`'s `link_exists` and `TUNSETIFF`, root attaches
to it. It may belong to another user's persistent device. **Change:** set `IFF_TUN_EXCL` (0x8000) so
the ioctl fails with EBUSY if the name exists. That is the kernel-level fix for this TOCTOU.

#### H-16 · Medium: external commands resolved via `$PATH`, no timeout, inherited environment

`ops.rs:14-20`, `ops.rs:77-84`, `ops.rs:104-106`, `ops.rs:158-163`, `tun.rs:109-117`

In `serve --socket` run under sudo or a custom environment, `PATH` decides which `ip` or `nft` runs as
root. A hung `nft` blocks the session (and, see M-3, a tokio worker) forever.
**Change:** use netlink; failing that, absolute paths, `env_clear()`, and a timeout (kill on expiry).

#### H-17 · Low: the helper does not enforce the phone source address itself

`connection.rs:94-99`

The relay injects any valid IPv4 packet from the controller into the TUN and relies entirely on nft
`raw_prerouting`. The helper is the privilege boundary, and a one-line `src == phone_ip` check is
defence in depth against a botched or missing table (see H-3/H-8).

#### H-18 · Low: the sysctl allowlist is broader than the use

`ops.rs:22-34`, test `ops.rs:235`

It allows `net.ipv4.ip_forward`, `conf.all.*`, `conf.default.*` and `rp_filter`, and the test asserts
`conf.all.forwarding` is OK. Architecture §12 says Routedroid "never writes broad settings such as
`conf.all.proxy_arp`". Nothing exploits this today, but the allowlist exists precisely for when a bug
does. **Change:** allow only `conf.<validated non-all/default ifname>.{forwarding,proxy_arp}`.

#### H-19 · Low: `valid_ifname` is looser than the kernel's `dev_valid_name`

`helper-ipc/src/proto.rs:42-48`

It accepts `.`, `..`, leading `-`, `all` and `default`. With `sysctl_path` (H-6), `.`/`..` become path
components. `lan_if` must exist and `tun` must start with `phone`, so this is not exploitable today.
**Change:** reject `.`, `..`, `all`, `default`, a leading `-` and `/`, mirroring `dev_valid_name`.

#### H-20 · Low: journal and claims files are created with the default umask

`journal.rs:83-90`, `claims.rs:59-65`, `claims.rs:43-44`

They are world-readable under `StateDirectory` (0755), and `create_dir_all` uses the default mode.
**Change:** `UMask=0077` in the unit, plus explicit `mode(0o600)` / `DirBuilder::mode(0o700)`.

### Crash recovery / journal correctness

#### J-1 · High: a torn final line makes the journal permanently unparseable and blocks every helper start

`journal.rs:170-184`, test `journal.rs:285-294`

The comment at line 176 says "torn final write after a crash mid-line", but the code only skips *empty*
lines. A partial JSON line `bail!`s. The unit test asserts exactly that (`read_lines(...).is_err()` on a
torn tail), which contradicts its own comment "Torn last line is tolerated". A torn write is the classic
power-loss case. After it: `open_sessions` → `?` fails, so `check` fails, so `ExecStartPre` refuses
forever. `cleanup` fails too (J-2), and there is no `--force`/repair path.
**Change:** tolerate an unparseable *last* line when it lacks a terminating `\n` (treat it as never
written). Keep failing on garbage in the middle. Add a documented `routedroid-helper repair` for manual
resolution.

#### J-2 · High: one bad journal file blocks cleanup and check for all other orphans

`journal.rs:218-232` (`open_sessions` uses `read_lines(&p)?` per file), `recovery.rs:15-16`

A single unparseable or unreadable file makes `orphaned_sessions` return `Err` before any other orphan
is replayed. One corrupt file leaves every other crashed session's nft tables and claims in place.
**Change:** collect per-file results, replay everything replayable, report the bad ones, and exit
non-zero.

#### J-3 · High: rollback failure still resolves (deletes) the journal

`session.rs:112-117`

On `sysctl_read` failure: `rollback(...)` returns `false` when an undo failed, the result is ignored,
and `let _ = journal.resolve()` removes the journal anyway. The un-undone mutations then have no record,
so neither `cleanup` nor `check` will ever see them. Compare lines 131-139, which do check `first && rest`.
**Change:** resolve only if the rollback fully succeeded, and share one rollback path for both error
branches.

#### J-4 · Medium: `Journal::create` has an unlocked window that a concurrent `cleanup`/`check` can hit

`journal.rs:88-99`

The sequence is `create_new` first, `flock` second. In between, another instance's `ExecStopPost`
`cleanup` sees an unlocked, empty, unresolved journal. It opens and locks it, finds no entries,
`resolve()`s it (unlinking the file), and our session then writes its whole WAL to an unlinked inode:
a crash after that leaves no record. A concurrent `ExecStartPre check` in that window refuses a
legitimate new instance.
**Change:** create `<session>.journal.tmp` with `O_EXCL`, `flock` it, then `rename` it into place (the
flock follows the inode). Or use `O_TMPFILE` + `linkat`.

#### J-5 · Medium: `Journal::open` reads the lines before taking the lock, and never checks that the file is still linked

`journal.rs:102-113`

Race between two cleanups: A reads the lines and opens an fd. B, which holds the lock, replays,
`resolve()`s and unlinks. A's lock then succeeds on the unlinked inode, and A replays with stale lines.
For `Sysctl` that means `claims::release` runs twice for the same session. That is harmless for this
session's holder, but it re-runs `ip link del`/`nft delete` by name, which is the same hazard as H-3.
**Change:** lock first, then `fstat` and check `nlink > 0` and that the inode matches the path, then read.

#### J-6 · Medium: the journal has no schema version

`journal.rs:52-69`

Architecture §17 says "Invalid or newer schemas fail closed". There is no version field, and an older
helper reading a newer journal would mis-parse or ignore new `Op` variants, because
`#[serde(untagged)] Line` swallows shape errors into the "unparseable" path.
**Change:** make the first line `{"journal":1}` and refuse unknown versions explicitly.

#### J-7 · Medium: `?` on journal writes during apply skips rollback

`session.rs:121,143`

If `journal.pending`/`journal.done` fails (ENOSPC, EIO), `start` returns with the kernel mutated and no
in-process undo. The journal fd is dropped and the file stays (good for cleanup), but in `--socket` mode
nothing runs cleanup until a restart that then refuses to start (see H-4).
**Change:** route every error after the first mutation through the same rollback path.

#### J-8 · Medium: `cleanup` treats every `Journal::open` error as "another instance has it" and succeeds

`recovery.rs:24-30`

A permission error or I/O error is logged at `info` as "skipping", and `cleanup` returns Ok, so
`ExecStopPost` reports success with orphans left behind. **Change:** skip only on `EWOULDBLOCK` from
`flock`, and fail on everything else.

#### J-9 · Medium: claims can never be garbage-collected

`claims.rs:69-111`

If a journal is ever lost (J-3, J-4) or deleted by hand, its session stays in `holders` forever, so the
LAN interface's `proxy_arp`/`forwarding` are never restored. Nothing cross-checks holders against
existing journals.
**Change:** in `cleanup`, drop holders whose session has no journal (live or orphaned) and then apply the
last-holder rule.

#### J-10 · Low: the `Sysctl.prev`/`new` journal fields are dead data

`journal.rs:17-21`, `session.rs:78-90`, `session.rs:106-117`

`prev` is "informational"; `new` is always `"1"`. `session::start` performs a `sysctl_read` for `prev`
and aborts the session if it fails, purely to record something nobody reads.
**Change:** reduce `Op::Sysctl` to `{ key }` (or typed `{ ifname, leaf }`) and drop the extra read and
its failure path (which is where J-3 lives).

#### J-11 · Low: `unresolved` is O(n²) and does not validate phase ordering

`journal.rs:190-203`

A `Pending` after an `Undone` for the same seq, or a `Done` without `Pending`, is accepted silently.
Corrupt-but-parseable journals should fail closed. Use a `BTreeMap<u32, (Phase, Op)>` and validate the
transitions.

#### J-12 · Low: `Journal::session()` returns `""` for a non-UTF-8 stem

`journal.rs:79-81`

`claims::holds("")` / `release("")` then match nothing and silently "succeed". Store the session id in
the struct instead of re-deriving it from the path.

### Concurrency / async (phase0-helper, helper-ipc)

#### M-1 · Medium: blocking work inside async tasks

`connection.rs:55` (`session::start`: fsyncs plus a dozen `Command::output()` calls),

`connection.rs:112` (`active.stop`), `claims.rs:47-51` (blocking `flock`), `serve.rs:25` (`check`)
In `serve --socket` multi-session mode, starting or stopping one session stalls a tokio worker.
Packets for other sessions are delayed, and the blocking `LockExclusive` can hold a worker for as long
as another process holds the claims lock.
**Change:** use `spawn_blocking` for plan apply and undo, or run each session on its own thread (the
relay is two fds).

#### M-2 · Low: `--once` and systemd mode behave differently from multi mode

`serve.rs:36-47`

Accept errors `continue` in a tight loop with no back-off (e.g. EMFILE spins at 100% CPU). **Change:**
back off on repeated accept errors.

#### M-3 · Low: `SeqPacket::connect` is nonblocking before `connect()`

`helper-ipc/src/seqpacket.rs:18-27,33-37`

With a full backlog (`listen(4)`, `seqpacket.rs:112`), a nonblocking Unix connect returns EAGAIN as a
hard error. **Change:** connect in blocking mode and switch to nonblocking afterwards, or wait for
writability. Raise the backlog.

#### M-4 · Low: the TUN read `n == 0` case is inconsistent with phase0-tunnel

`connection.rs:84`

The helper `continue`s on a 0-byte TUN read; phase0-tunnel treats it as "device gone" and stops. If it
ever happens, the helper spins. Break (or error) instead.

### helper-ipc wire / API design

#### P-1 · Medium: no protocol version or handshake between daemon and helper

`helper-ipc/src/proto.rs:17-31`

The helper is installed separately (`/usr/local/libexec`) from the user's daemon build. A mismatched
pair fails with confusing serde errors or, worse, with subtle semantic drift.
**Change:** add `Request::Hello { version }` → `Reply::Hello { version }` as the mandatory first
exchange, and add a `HELPER_IPC_VERSION` constant.

#### P-2 · Medium: framing is hand-rolled in three places

`phase0-helper/src/connection.rs:13-17,86-88,113-118`, `phase0-helper/src/client.rs:32-47`,

`routedroidd/src/host_network/client.rs:31-47,80-84`
The crate says "Shared by both binaries so the wire cannot drift", but it exports only constants. Each
side builds `[KIND_*][payload]` by hand, and the daemon's `out[1..=pkt.len()]` panics if `pkt.len() >
65536`.
**Change:** export `enum Datagram<'a> { Control(Request|Reply), Packet(&'a [u8]) }` with `encode_into`
and `decode`, plus `SeqPacket::send_control` and `recv_datagram`.

#### P-3 · Low: stringly-typed error codes

`proto.rs:37` (`Error { code: String }`); the codes used are `"invalid"`, `"bad_request"`,

`"start_failed"`, `"out_of_state"` and `"stop_failed"` (`connection.rs`).
**Change:** `#[serde(rename_all="snake_case")] enum ErrorCode`.

#### P-4 · Low: the default socket path is duplicated and carries the "phase0" name

`phase0-helper/src/main.rs:23` and `routedroidd/src/host_network/client.rs:14`

**Change:** move `DEFAULT_SOCKET` into helper-ipc and rename it (`/run/routedroid/helper.sock`).

#### P-5 · Nit: the size constants are off by one and oversized

`proto.rs:13-16`

`MAX_DATAGRAM = 65536 + 1`, but the largest IPv4 packet is 65535, so the maximum datagram is 65536. The
helper also caps the MTU at 9000, so every relay allocates 64 KiB buffers for 9 KiB packets. Use
`MSG_TRUNC` in `recv` for exact truncation detection instead of the "buffer filled" heuristic
(`seqpacket.rs:57-60`).

#### P-6 · Nit: `Request::Stop`'s doc says "then exit"

`proto.rs:28`

That is false in multi-session `serve --socket` mode; the connection ends, not the process.

#### P-7 · Nit: no `#[serde(deny_unknown_fields)]` on `Request`

A typo'd field (`phoneip`) gives a generic "expected control JSON" instead of a precise error, because
`parse_control` discards the serde error (`connection.rs:125-130`). Return the serde message in
`bad_request`.

### phase0-dhcp (to be promoted, see §0)

#### D-1 · Medium: misaligned `cmsghdr` access is undefined behaviour

`phase0-dhcp/src/sock.rs:276-305`

`cmsg_buf: [u8; 64]` has alignment 1. `CMSG_FIRSTHDR` returns `*mut cmsghdr` into it, and
`(*c).cmsg_level` dereferences a possibly misaligned pointer, which is UB in Rust even where x86 allows
it. **Change:** use an aligned buffer (`[u64; 8]` or a `#[repr(C, align(8))]` wrapper), or `nix`/
`rustix` `recvmsg` with `RecvAncillaryBuffer`.

#### D-2 · Medium: replies are not matched on client identity; only XID and chaddr are checked

`client.rs:290-344`

Architecture §6.1 requires verifying "the XID, client identity, hardware address fields, server
selection". Because `chaddr` is the *host's own MAC* (shared with the PC's own DHCP client), option 61
echo (when the server includes it) is the only thing distinguishing our lease from the host's.
`lease_from_ack` also never checks that `yiaddr` differs from the host's own address or from another
Routedroid lease. **Change:** verify option 61 when present, and reject `yiaddr` equal to any
host-owned address.

#### D-3 · Medium: the client-id is the raw operator string, not the hashed identity the spec requires

`main.rs:47-52`, `dhcp.rs` `Identity::new`

Architecture §6.1: `client-id = routedroid:<stable-device-id>:<interface-stable-id>` with the serial
hashed. The probe sends whatever it is given. On promotion, derive it from (serial hash, interface
id) inside the crate. Do not accept free-form strings.

#### D-4 · Low: `Lease::save` is not durable

`client.rs:77-83`

It writes the tmp file and renames it without `fsync` of the file or the directory. The lease is exactly
the state architecture §17 wants durable. Also `.tmp` is a fixed sibling name, so two writers collide.

#### D-5 · Low: the renew/rebind timers use wall-clock `unix_now()`

`client.rs:86-88` and `hold` (~`client.rs:560-640`)

A clock step (NTP, suspend) mis-times renewal. Use `Instant`/`CLOCK_BOOTTIME` for scheduling and keep
wall-clock only for the persisted record.

#### D-6 · Low: `random_u32` falls back to time^pid

`sock.rs:315-325`

An XID predictable in this way aids spoofed OFFER/ACK injection. Fail hard instead; `getrandom` cannot
fail on supported kernels.

#### D-7 · Nit: truncated frames return `Ok((0, meta))`

`sock.rs:307-311`

The caller has to know that 0 means "dropped". Return a distinct variant.

#### D-8 · Nit: file sizes

`client.rs` is 732 lines, `dhcp.rs` 635, `packet.rs` 381, `sock.rs` 330 and `main.rs` 290, all far above

the ≲150-line preference. Split the transaction state machine, the lease, ARP, the hold loop, the option
codec, BPF, and move tests to sibling `tests.rs`.

### phase0-tunnel (delete; findings recorded for completeness)

#### T-1 · High (as dead code): it targets an app and protocol that no longer exist

`phase0-tunnel/src/adb.rs:9-10` (`dev.routedroid.phase0/...`), `auth.rs:86` (`rd-p0-auth`),

`auth.rs:27` (`RDB0`), `frame.rs:18` (version 0)
Its `run` mode cannot work against the current app. It is built by default in the workspace
(`host/Cargo.toml:10`) and uses CI time, attack surface (it creates TUNs as root) and reviewer
attention. **Change:** delete the crate.

#### T-2 · Medium: it duplicates `routedroid-proto` wholesale

`frame.rs` (450 lines) ≈ `routedroid-proto/src/frame/*`, `auth.rs` ≈ `routedroid-proto/src/auth/*`,

`messages.rs`, `ipv4.rs`, and the session machine ≈ `routedroid-proto/src/state` plus routedroidd's
`session`. Two implementations of the same concept with different version bytes is exactly the drift the
fixtures exist to prevent.

#### T-3 · Medium (security, moot once deleted): HELLO `device_port` is not bound to the actual reverse port

`session.rs` HELLO arm (~line 170-190)

The transcript uses the port *claimed by the phone*. The host never compares it with its own
`device_port`, so the "reverse-port identity" binding of architecture §8.2 is void here. (routedroidd
does check `expected_device_port`: `routedroidd/src/session/config.rs:11-13`.) With `--no-adb` and no
`--session`, the session id is not enforced either (`main.rs:82-85,179`).

#### T-4 · Low: the TUN code is duplicated verbatim with phase0-helper

`phase0-tunnel/src/tun.rs:1-126` vs `phase0-helper/src/tun.rs` ("Copied from phase0-tunnel").



#### T-5 · Nit: `expect()` in protocol state handling on peer-driven paths

`session.rs:207-208` (`expect("secret present while Authenticating")`)

It is correct by the state invariant, but a state-machine refactor turns it into a remote-triggerable
panic. Moot if deleted.

### Duplication across crates

#### X-1 · Medium: the IPv4 sanity check is implemented three times with different rules

`phase0-helper/src/connection.rs:19-21` (`ipv4_ok`: no `IHL*4 <= len` check),

`phase0-tunnel/src/ipv4.rs:34-56`, `routedroid-proto/src/ipv4/mod.rs:18` (`check`)
The helper should call `routedroid_proto::ipv4::check`. That means adding a `routedroid-proto`
dependency to the helper, or moving the check into a tiny shared `routedroid-packet` crate.

#### X-2 · Low: the Internet checksum and ICMP echo builders are duplicated

`phase0-helper/src/client.rs:54-85`, `phase0-tunnel/src/fake_client.rs:25+`,

`phase0-dhcp/src/packet.rs:40` (a fourth checksum in `protocol/tools/gen-fixtures.py:37`)

#### X-3 · Low: the protocol is implemented four times

Rust `routedroid-proto`, Kotlin, `gen-fixtures.py`, and `integration-tests/phase1/fake_host.py:25-52`
(`frame`, `transcript`, `proof`, bootstrap record).
**Change:** `fake_host.py` should import the builders from `gen-fixtures.py` (turn it into an importable
module plus a `__main__`), or at least assert its builders against `fixtures/*.json` at start-up.

#### X-4 · Low: tracing-subscriber init is copy-pasted in four `main.rs` files

phase0-helper, phase0-tunnel, phase0-dhcp and routedroidd. The helper logs to stderr under journald
without `with_ansi(false)`, so escape codes can land in the journal **(unverified: tracing-subscriber
may auto-detect)**.

### Code style, naming, structure (phase0-helper + helper-ipc)

#### N-1 · Medium: "phase0" naming on production components

Package `phase0-helper`, binary `phase0-helper`, units `routedroid-phase0-helper{.socket,@.service}`,
socket `/run/routedroid/phase0-helper.sock`, journal `/var/lib/routedroid/phase0-journal`, module
header "Phase 0 helper-gate spike … Throwaway quality" (`main.rs:1-2`), `Cargo.toml:4` description
"spike". routedroidd depends on all of these at runtime.
**Change:** use `routedroid-helper` everywhere, and rename the `helper-ipc` directory to
`routedroid-helper-ipc` to match its package name (the other crates' directories match their names).

#### N-2 · Medium: the test client is compiled into the privileged binary

`main.rs:59-86`, `client.rs` (179 lines: ICMP bench, self-SIGKILL)

The root helper ships an unprivileged test tool with a `kill(getpid(), SIGKILL)` path. **Change:** move
it to `integration-tests/` as a separate dev binary (or an `examples/` target), and drop `libc` from the
helper once the hooks go.

#### N-3 · Low: global mutable configuration

`claims.rs:19-28` (`static DIR: OnceLock<PathBuf>`, `init()` silently ignores a second call)

The rest of the codebase just moved from free functions to objects (commits 32a6c89, 2af0f0e). Make
`Claims { dir }` a value passed into `ops`/`session`/`recovery`. This also makes `acquire`/`release`
unit-testable against a scratch directory; today only `read_claim`/`write_claim` are tested.

#### N-4 · Low: `ops.rs` mixes four concerns and is 258 lines

It holds the command runner, sysctl, the nft ruleset text, and link and route probing. Split it into
`kernel/{run,sysctl,nft,route,link}.rs` with tests in `tests.rs`. `journal.rs` (302 lines) has an
inline `mod tests`; move that to `journal/tests.rs`. Same for `ops.rs` and `helper-ipc/proto.rs`.
`client.rs` is 179 lines.

#### N-5 · Low: stringly-typed `Op` fields

`journal.rs:14-37`

`family: String` is always `"inet"` and is then re-validated (`ops.rs:100-102`); `name: String` is
re-validated; `key: String` is re-parsed (H-6). Use types: `NftTable { tun: IfName }`,
`Sysctl { ifname: IfName, leaf: SysctlLeaf }`, with an `IfName` newtype validated once at the IPC
boundary.

#### N-6 · Low: `Plan` has public fields and `session: String` is minted in `connection.rs`

`connection.rs:36` (`format!("s{}-{}", pid, nanos)`), `nanos()` falls back to 0 on clock error

(`connection.rs:132`), which gives colliding session ids. Session-id minting belongs in `session`/
`journal`. Use a random 64-bit id.

#### N-7 · Nit: `use` statements interleaved with `mod` declarations

`main.rs:4-14`

Also `pub const` items in a binary crate (`main.rs:23-25`) have no reason to be `pub`.

#### N-8 · Nit: lines rustfmt cannot reach inside `tokio::select!`

`connection.rs:91` (157 columns) and `:103`, `client.rs:138` (167 columns)

Move the arm bodies into functions so rustfmt governs them.

#### N-9 · Nit: `let _ = tun;` does nothing

`client.rs:109`

It destructures `tun` only to discard it. Don't bind it.

#### N-10 · Nit: inconsistent error rendering

`connection.rs:40` uses `e.to_string()` (loses the anyhow chain), `:58` uses `{e:#}`, `:118` uses

`e.to_string()`.

#### N-11 · Nit: comment inaccuracies

`session.rs:77-79` says `prev` is "filled in at apply time (the TUN's do not exist yet)"; this is really

about the plan being built before the TUN exists, and the field is dead anyway (J-10). `tun.rs:24`
"Create (or attach to)" documents the bug in H-15. `journal.rs:176` describes the torn-line handling
that isn't there (J-1).

### Workspace / build configuration

#### W-1 · Medium: no lints configured anywhere

There is no `[workspace.lints]` and no `#![deny(...)]`/`#![forbid(unsafe_code)]` in any crate. The helper
has 10 `unsafe` blocks: `tun.rs` (open, ioctl, CStr, read, write) and `session.rs` (kill). Suggested:

```toml
[workspace.lints.rust]
unsafe_code = "deny"            # allow per-module where needed (tun, sock) with a SAFETY review
missing_debug_implementations = "warn"
[workspace.lints.clippy]
unwrap_used = "deny"            # tests: allow
expect_used = "warn"
pedantic = { level = "warn", priority = -1 }
```

`cargo clippy -W clippy::pedantic` on the four in-scope crates gives about 200 warnings, among them 6
`isize → usize` sign-loss casts on syscall results, 9 raw-pointer `as` casts, and truncating
`usize → u16/u32` casts. Default clippy is clean.

#### W-2 · Medium: no CI at all

There is no `.github/` or any other CI. Nothing runs `cargo fmt --check`, `clippy -D warnings`,
`cargo test`, a fixture-regeneration diff (`gen-fixtures.py && git diff --exit-code protocol/fixtures`),
`shellcheck integration-tests/**/*.sh`, or the unprivileged userns rigs (`helper-kill.sh userns`,
`phase2/helper-multi.sh` both run without root and could run in CI).

#### W-3 · Low: no `[workspace.dependencies]`

tokio, serde, serde_json, anyhow, clap, tracing, tracing-subscriber, libc and rustix are versioned
separately in each manifest. Use `[workspace.dependencies]` and `dep.workspace = true`.

#### W-4 · Low: no `rust-version`

The code uses `c"..."` literals (1.77), `is_some_and` (1.70) and `io::Error::other` (1.74), and the
edition is 2021 while 2024 is available. Pin `rust-version` in `[workspace.package]`.

#### W-5 · Low: `tokio = { features = ["full"] }` in the helper, tunnel and dhcp crates

The helper needs `rt-multi-thread, macros, net, io-util, signal, time` at most; routedroidd already
lists its features explicitly. `getrandom = "0.2"` in phase0-tunnel is a major version behind.
`libc` + `rustix` + `socket2` overlap: rustix can do `open`, `kill`, `fcntl` and `ioctl` safely,
eliminating most of the helper's `unsafe`.

#### W-6 · Nit: `rustfmt.toml`'s `use_small_heuristics = "Max"` with `max_width = 120`

This yields very dense one-liners (e.g. `session.rs:74`, the `Plan` constructor on one 120-column line).
It is a matter of taste, but it works against the "readable, small files" goal. Consider the defaults
with `max_width = 110`.

### systemd units and install.sh

#### S-1 · Medium: `install.sh` installs from a relative release build and gives the user root-network powers silently

`install.sh:8,22-27`

It adds `$SUDO_USER` to `routedroid`. Given H-1/H-2, that grants LAN-wide ARP hijack capability, and
the script says nothing about it. At minimum, print what group membership permits. Fix H-1/H-2 first.

#### S-2 · Low: the `install.sh` pipeline can fail spuriously under `pipefail`

`install.sh:36` (`systemctl status --no-pager … | head -5`)

If `head` closes early, `systemctl` gets SIGPIPE and the script exits non-zero after a successful
install **(unverified: depends on output size and buffering)**. Use `systemctl is-active` or
`--lines=0`.

#### S-3 · Low: `install.sh` unquoted expansions and uninstall hygiene

`$UNIT` is unquoted in `install.sh:13-16,31,35` (a constant today). Uninstall leaves the

`/var/lib/routedroid` journals, the claims and the group behind (documented in the echo), but it does
not run `cleanup` first to make sure no orphan remains. `rm -rf /run/routedroid` races with instances
still in `ExecStopPost`.

#### S-4 · Low: `After=network.target` on a per-connection template is meaningless

`@.service:10`. Also `Requires=` should be `BindsTo=`-free as is; fine. Remove the `After=`.



#### S-5 · Low: no SIGTERM handling in `serve`

`systemctl stop` of an instance kills it with the default action, so there is no graceful undo and the

system relies on `ExecStopPost`. That is acceptable by design, but a TERM handler that runs `stop()`
would make shutdowns clean and make `ExecStopPost` a real backstop rather than the normal path.

### Tests

#### E-1 · Medium: the helper's logic has almost no unit tests

There are 6 unit tests in phase0-helper (journal round-trip, sysctl allowlist, nft table names, nft rule
text, 2 for claims files) and 2 in helper-ipc. Untested in-process: `Plan::build` validation (H-9/H-10
would have been caught), `session::start` rollback paths (J-3 would have been caught),
`recovery::cleanup` phase handling, `connection::serve` protocol handling (H-4), torn-tail parsing
(J-1). Everything relies on the root/userns shell rigs.
**Change:** put the kernel behind a trait (`Kernel { apply, probe, undo }`) with a fake implementation,
and unit-test plan, start, rollback, stop and recovery exhaustively, including the crash matrix.

#### E-2 · Medium: the kill matrix's "client saw the helper vanish" check is vacuous

`integration-tests/phase0/helper-kill.sh:125-127`

`grep -qE 'helper closed|STARTED'`: STARTED is always present for `active`/`undo*`/`done:route*`, so the
check passes regardless. Grep for `helper closed` only.

#### E-3 · Medium: missing scenarios

None of the rigs covers: a helper crash while *another* session is live (H-3); TUN-name or address
reuse between a crash and its cleanup (H-3); a torn journal line (J-1); a corrupt second journal (J-2);
IPC send failure after start (H-4); a VLAN `lan_if` (H-6); a phone address equal to a host secondary
address (H-9) or the gateway (H-1). The phase2 test "one session's crash leaves the other intact"
(`phase2/helper-multi.sh:222-231`) kills the *client*, not a helper session, so its title overstates it.

#### E-4 · Low: the shell rigs are inconsistent in rigor

- `set -euo pipefail` in helper-kill, lan-proxyarp, phone-*; `set -u` only in `phase1/emulator-userns.sh:15`
  and `phase2/helper-multi.sh:13`; `set -u -o pipefail` in the netns-* scripts. Pick one convention
  (`-u -o pipefail` plus explicit `check` is reasonable for check-style scripts) and apply it everywhere.
- `mktemp -d /tmp/...` artifacts are never removed on success (helper-kill, helper-multi,
  emulator-userns, phone-*). Remove them on pass and keep them on fail.
- `$NS` and `$HELPER` are word-split command strings (`helper-kill.sh:25,28`, `helper-multi.sh:23,25`).
  Use arrays (`NS=(nsenter -t "$NSPID" -U -n --preserve-credentials)`, then `"${NS[@]}" ip ...`).
- Fixed `sleep 0.5` after `unshare … &` in every script is a race. Poll for
  `/proc/$NSPID/ns/net` != the parent's instead.
- `phase2/helper-multi.sh:223-224`: the `started a2`/`started b2` results are not checked.
- `phase0/netns-tunnel.sh:16` defaults to the *debug* binary; the other scripts use release.
- Nobody runs `shellcheck` (see W-2).

#### E-5 · Low: root script writes a predictable `/tmp` path

`integration-tests/phase0/lan-proxyarp.sh:19` (`LOG=${LOG:-/tmp/routedroid-lan-tunnel.log}` as root)

This is a symlink-clobber candidate, mitigated by `fs.protected_symlinks`. Moot if the script is deleted.
The same script's cleanup (`:63-64`) removes *any* `tcp:9000` reverse mapping, not just its own.

#### E-6 · Low: the phase0 phone scripts reference the removed app

`phase0/phone-bootstrap.sh:16,56,61` (`dev.routedroid.phase0`, `Phase0VpnService`) and

`phase0/phone-multialias.sh:53-54`. They cannot pass against the current app. Delete them with
phase0-tunnel.

#### E-7 · Low: committed bytecode

`integration-tests/phase0/__pycache__/fake_android.cpython-314.pyc` is tracked (since commit 1446620),

and `.gitignore` has no `__pycache__/` or `*.pyc`. `git rm` it and add both patterns.

#### E-8 · Low: `fake_host.py` asserts with `assert`

`integration-tests/phase1/fake_host.py` throughout

`python3 -O` strips every assertion and all cases "pass". Also, sockets are not closed on failure (no
`with`); `bootstrap()` unconditionally removes `tcp:17900` reverse mappings (`:58`); `recv_frame`
trusts the peer's `n` with no bound (`:37-41`); `except Exception` at `:278` hides `KeyboardInterrupt`
handling issues (it is fine, but consider `BaseException` for exit codes).

### Protocol tooling and fixtures

#### F-1 · Low: fixture drift is guarded only by convention

`protocol/tools/gen-fixtures.py`

It is deterministic: I re-ran it and got no diff. But nothing enforces that the committed fixtures match
the generator (see W-2). The Rust side consumes them (`routedroid-proto/src/lib.rs:29-32`), and so does
the Kotlin side (`android/protocol/build.gradle.kts:17`), which is good.

#### F-2 · Low: the generator's docstring overclaims

`gen-fixtures.py:2` says "Generate protocol/fixtures/*.json from protocol/version-1.md". It does not read

the spec; the values are hand-transcribed. Say "per protocol/version-1.md".

#### F-3 · Nit: `ipv4_checksum` raises on odd-length input

`gen-fixtures.py:37-41` (`struct.unpack(">%dH" % (len(b)//2), b)` raises `struct.error`)

`icmp_echo(payload=b"\x00")` would crash. Pad to even length.

#### F-4 · Nit: `ip_packet_min` has an invalid header checksum (0)

`gen-fixtures.py:110-111`

Harmless, because no side verifies IPv4 checksums, but a comment should say so to prevent someone
"fixing" a validator against it.

#### F-5 · Nit: `states.json` is emitted from a hand-written dict, and `routedroid-proto/src/state/mod.rs:44-60` is a second hand-written copy

They agree today. This is fine because the Rust test checks one against the other; keep that test
mandatory in CI.
