//! Android-side tools reached through `adb shell`, one method per tool, so
//! callers never build shell command lines themselves.

use super::AdbDevice;
use routedroid_ipc::fault::{Fault, Kind, Result};

impl AdbDevice {
    /// `content write --uri URI` with `bytes` on stdin: the only way to hand
    /// a payload to a content provider without it appearing in a command line.
    pub async fn content_write(&self, uri: &str, bytes: &[u8]) -> Result<()> {
        let (stdout, stderr) = self.shell(&["content", "write", "--uri", uri], Some(bytes)).await?;
        // `content` exits 0 even on provider errors; it prints them (to either stream) instead.
        let out = format!("{stdout}{stderr}");
        if !out.trim().is_empty() {
            return Err(Fault::msg(Kind::Adb, format!("content write reported: {}", out.trim())));
        }
        Ok(())
    }

    /// `am start -n COMPONENT` with string extras (`--es KEY VALUE`).
    pub async fn am_start(&self, component: &str, extras: &[(&str, &str)]) -> Result<()> {
        let mut args = vec!["am", "start", "-n", component];
        for (key, value) in extras {
            args.extend(["--es", key, value]);
        }
        let (out, err) = self.shell(&args, None).await?;
        // `am start` exits 0 even when the component is missing; it prints
        // `Error: ...` or `Error type N` lines instead. Warnings (an intent
        // delivered to the running activity) are success.
        let failure = out.lines().chain(err.lines()).map(str::trim).find(|l| l.starts_with("Error"));
        if let Some(line) = failure {
            return Err(Fault::msg(Kind::Adb, format!("am start reported: {line}")));
        }
        Ok(())
    }
}
