# Contributing and testing

The source is at <https://github.com/NoamCohen48/routedroid>.

## The repository

| Path | What |
|---|---|
| `host/` | the Rust workspace: daemon, CLI, TUI, root helper, DHCP client, wire protocol, IPC crates, fuzz targets; `install.sh` and `packaging/` (.deb, .rpm) |
| `android/` | the app, its protocol library, and a hostile test app |
| `protocol/` | the wire protocol and its golden fixtures |
| `integration-tests/` | rigs: namespace rigs for the helper, emulator rigs, and a KVM lab with a real phone |
| `docs/site/` | this site |
| `.docs/` | architecture, decisions, implementation plan, code review |

## Tests

```sh
cd host && cargo test --workspace
cd android && ./gradlew :protocol:test :app:testDebugUnitTest
integration-tests/helper/kill-matrix.sh       # and the other rigs there; no root needed
```

- **Unit tests** cover each crate; the control API's JSON is pinned by golden tests.
- **Namespace rigs** (`integration-tests/helper/`) run the real helper in user namespaces,
  without root: crashes at every stage, several sessions at once, and DHCP leases.
- **Emulator rigs** (`integration-tests/emulator/`) connect an Android emulator.
- **The VM lab** (`integration-tests/vm/`) runs KVM guests with real root, systemd and a USB
  phone passed through: install, ufw, packages, unplugging, the app install, the TUI and the
  CLI. Its README lists the latest results.

CI runs the unit tests, the namespace rigs, and the packages' install, upgrade and removal
in Debian, Ubuntu and Fedora containers. A `vX.Y.Z` tag drafts a release.

## This site

The site is built with [mdBook](https://rust-lang.github.io/mdBook/) from `docs/site/`:

```sh
mdbook serve docs/site      # http://localhost:3000, rebuilt on save
```

A push to `main` that changes `docs/site/` publishes it to GitHub Pages.
