//! A stand-in `adb` executable: a shell script over a state directory, so
//! the daemon runs its real adb code (argument building, output parsing,
//! the reverse-port dance) against a phone that is only files.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adb::Adb;

const SCRIPT: &str = r#"#!/bin/sh
D=$(dirname "$0")
case "$1" in
devices) echo "List of devices attached"
         while read -r s; do echo "$s device model:Fake"; done < "$D/serials"; exit 0;;
track-devices) exit 1;;
-s) S=$2; shift 2;;
*) exit 2;;
esac
R="$D/reverse.$S"; touch "$R"
case "$1 $2" in
"reverse --list") cat "$R";;
"reverse --no-rebind") grep -q " $3 " "$R" && { echo "error: cannot rebind" >&2; exit 1; }
                       echo "$S $3 $4" >> "$R";;
"reverse --remove") grep -v " $3 " "$R" > "$R.new"; mv "$R.new" "$R";;
"shell content") cat > "$D/record.$S";;
"shell am") echo "Starting: Intent { cmp=$5 }";;
*) echo "fake adb: unknown $*" >&2; exit 2;;
esac
"#;

pub struct FakeAdb {
    dir: PathBuf,
}

impl FakeAdb {
    /// A phone for each serial, attached over USB.
    pub fn new(dir: &Path, serials: &[&str]) -> Self {
        let script = dir.join("adb");
        std::fs::write(&script, SCRIPT).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let list: String = serials.iter().map(|s| format!("{s}\n")).collect();
        std::fs::write(dir.join("serials"), list).unwrap();
        Self { dir: dir.into() }
    }

    pub fn adb(&self) -> Adb {
        Adb::new(
            self.dir.join("adb").to_str().unwrap(),
            Duration::from_secs(5),
        )
    }

    /// The bootstrap record the daemon wrote to the app's provider.
    pub fn record(&self, serial: &str) -> Option<Vec<u8>> {
        std::fs::read(self.dir.join(format!("record.{serial}"))).ok()
    }

    /// The `adb reverse` mappings, as `(device port, host port)`.
    pub fn reverse(&self, serial: &str) -> Vec<(u16, u16)> {
        let text = std::fs::read_to_string(self.dir.join(format!("reverse.{serial}")));
        text.unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace().skip(1);
                let port = |w: Option<&str>| w?.strip_prefix("tcp:")?.parse().ok();
                Some((port(words.next())?, port(words.next())?))
            })
            .collect()
    }
}
