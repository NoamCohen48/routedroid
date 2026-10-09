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
  `integration-tests/emulator/all.sh` runs them all.
- **The VM lab** (`integration-tests/vm/`) runs KVM guests with real root, systemd and a USB
  phone passed through: install, ufw, packages, unplugging, the app install, the TUI and the
  CLI. Its README lists the latest results.

CI runs the unit tests, the namespace rigs, and the packages' install, upgrade and removal
in Debian, Ubuntu and Fedora containers. Its `e2e` workflow runs the emulator rigs on an
Android 14 emulator, on every pull request and nightly. A `vX.Y.Z` tag drafts a release.

## This site

The site is built with [mdBook](https://rust-lang.github.io/mdBook/) from `docs/site/`:

```sh
mdbook serve docs/site      # http://localhost:3000, rebuilt on save
```

A push to `main` that changes `docs/site/` publishes it to GitHub Pages. Each release publishes it again,
with `apt/`, an apt repository of every release's `.deb`, and `download/`, the latest
release's files. The repository is signed with the `APT_SIGNING_KEY` secret, which
`host/packaging/apt-key.sh` makes once; without it the site has no `apt/`.
`integration-tests/packages/apt-repo.sh` builds one from `host/target/packages` and installs
from it in Debian and Ubuntu containers, as CI does.
