//! Thin wrappers around the `adb` CLI: reverse mapping, activity launch, and
//! cautious reverse-mapping removal.

use anyhow::{bail, Context, Result};
use std::process::Command;
use tracing::{info, warn};

pub const BOOTSTRAP_COMPONENT: &str = "dev.routedroid.phase0/.BootstrapActivity";

fn adb(serial: &str) -> Command {
    let mut c = Command::new("adb");
    c.args(["-s", serial]);
    c
}

fn run(mut cmd: Command) -> Result<String> {
    let desc = format!("{cmd:?}");
    let out = cmd.output().with_context(|| format!("spawn {desc}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        bail!(
            "{desc} failed ({}): {}{}",
            out.status,
            stdout.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(stdout)
}

/// `adb -s SERIAL reverse tcp:DEVICE_PORT tcp:HOST_PORT`
pub fn reverse_add(serial: &str, device_port: u16, host_port: u16) -> Result<()> {
    let mut c = adb(serial);
    c.args(["reverse", &format!("tcp:{device_port}"), &format!("tcp:{host_port}")]);
    run(c).map(|_| ())
}

/// Parse `adb reverse --list` output into (remote, local) pairs. Lines look
/// like `<serial-or-transport> tcp:9000 tcp:41234`.
pub fn parse_reverse_list(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let _id = it.next()?;
            let remote = it.next()?;
            let local = it.next()?;
            Some((remote.to_string(), local.to_string()))
        })
        .collect()
}

/// Whether the list output contains exactly our mapping (and no other mapping
/// for the same device port).
pub fn list_has_exactly(text: &str, device_port: u16, host_port: u16) -> bool {
    let remote = format!("tcp:{device_port}");
    let local = format!("tcp:{host_port}");
    let ours: Vec<_> = parse_reverse_list(text).into_iter().filter(|(r, _)| *r == remote).collect();
    ours.len() == 1 && ours[0].1 == local
}

/// Remove the reverse mapping only if `adb reverse --list` still shows exactly
/// our mapping; otherwise leave it alone and log.
pub fn reverse_remove_if_ours(serial: &str, device_port: u16, host_port: u16) {
    let mut c = adb(serial);
    c.args(["reverse", "--list"]);
    let list = match run(c) {
        Ok(l) => l,
        Err(e) => {
            warn!(error = %e, "could not list adb reverse mappings; leaving them untouched");
            return;
        }
    };
    if !list_has_exactly(&list, device_port, host_port) {
        warn!(device_port, host_port, list = %list.trim(), "reverse mapping is not ours any more; not removing");
        return;
    }
    let mut c = adb(serial);
    c.args(["reverse", "--remove", &format!("tcp:{device_port}")]);
    match run(c) {
        Ok(_) => info!(device_port, host_port, "removed adb reverse mapping"),
        Err(e) => warn!(error = %e, "failed to remove adb reverse mapping"),
    }
}

/// `adb shell am start -n dev.routedroid.phase0/.BootstrapActivity --es session S --ei device_port N`
pub fn launch_bootstrap(serial: &str, session: &str, device_port: u16) -> Result<()> {
    let mut c = adb(serial);
    c.args([
        "shell",
        "am",
        "start",
        "-n",
        BOOTSTRAP_COMPONENT,
        "--es",
        "session",
        session,
        "--ei",
        "device_port",
        &device_port.to_string(),
    ]);
    let out = run(c)?;
    // `am start` exits 0 even when the component is missing; surface that.
    if out.contains("Error") || out.contains("does not exist") {
        bail!("am start reported: {}", out.trim());
    }
    info!(component = BOOTSTRAP_COMPONENT, "launched bootstrap activity");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reverse_list_and_matches_exactly() {
        let text = "UsbFfs tcp:9000 tcp:41234\nUsbFfs tcp:9001 tcp:5\n";
        assert_eq!(
            parse_reverse_list(text),
            vec![("tcp:9000".into(), "tcp:41234".into()), ("tcp:9001".into(), "tcp:5".into())]
        );
        assert!(list_has_exactly(text, 9000, 41234));
        assert!(!list_has_exactly(text, 9000, 41235));
        assert!(!list_has_exactly(text, 9002, 41234));
        assert!(!list_has_exactly("", 9000, 41234));
        // Duplicate mappings for our device port: not "exactly ours".
        let dup = "X tcp:9000 tcp:41234\nX tcp:9000 tcp:1\n";
        assert!(!list_has_exactly(dup, 9000, 41234));
    }
}
