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
grep -qx "$S" "$D/serials" || { echo "error: device '$S' not found" >&2; exit 1; }
R="$D/reverse.$S"; touch "$R"
case "$1 $2" in
"reverse --list") cat "$R";;
"reverse --no-rebind") grep -q " $3 " "$R" && { echo "error: cannot rebind" >&2; exit 1; }
                       echo "$S $3 $4" >> "$R";;
"reverse --remove") grep -v " $3 " "$R" > "$R.new"; mv "$R.new" "$R";;
"shell content") cat > "$D/record.$S.new" && mv "$D/record.$S.new" "$D/record.$S";;  # whole, for the app's poll
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
        // Another test forking while this one wrote the script holds the
        // write descriptor until its child execs; running the script fails
        // with ETXTBSY until then. Wait that out once, here.
        for _ in 0..200 {
            let run = std::process::Command::new(&script).arg("devices").output();
            match run {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => break,
            }
        }
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

    /// Pull the cable: adb forgets the phone, and with its transport the
    /// reverse mappings and (as far as the daemon can tell) the record.
    pub fn unplug(&self, serial: &str) {
        let serials = self.dir.join("serials");
        let list = std::fs::read_to_string(&serials).unwrap();
        let kept: String = list
            .lines()
            .filter(|s| *s != serial)
            .map(|s| format!("{s}\n"))
            .collect();
        std::fs::write(serials, kept).unwrap();
        for file in [format!("record.{serial}"), format!("reverse.{serial}")] {
            let _ = std::fs::remove_file(self.dir.join(file));
        }
    }

    pub fn plug(&self, serial: &str) {
        use std::io::Write;
        let serials = self.dir.join("serials");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(serials)
            .unwrap();
        writeln!(file, "{serial}").unwrap();
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
