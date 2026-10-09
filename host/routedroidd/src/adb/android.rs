//! Android-side tools reached through `adb shell`, one method per tool, so
//! callers never build shell command lines themselves.

mod screen;

use std::time::Duration;

use super::AdbDevice;
use crate::fault::{Fault, Kind, Result};

/// How long each step that wakes the app may take (the record write starts
/// its process, the launch its activity). A phone just booted or just
/// updated can take over adb's usual 15 s for either. The app keeps the
/// record for 60 s, so both together stay inside that.
const APP_TIMEOUT: Duration = Duration::from_secs(25);

impl AdbDevice {
    /// `content write --uri URI` with `bytes` on stdin: the only way to hand
    /// a payload to a content provider without it appearing in a command line.
    pub async fn content_write(&self, uri: &str, bytes: &[u8]) -> Result<()> {
        let (stdout, stderr) = self
            .shell_with(
                &["content", "write", "--uri", uri],
                Some(bytes),
                APP_TIMEOUT,
            )
            .await?;
        // `content` exits 0 even on provider errors; it prints them (to either stream) instead.
        match content_failure(uri, &format!("{stdout}{stderr}")) {
            Some(message) => Err(Fault::msg(Kind::Adb, message)),
            None => Ok(()),
        }
    }

    /// `am start -n COMPONENT` with string extras (`--es KEY VALUE`).
    pub async fn am_start(&self, component: &str, extras: &[(&str, &str)]) -> Result<()> {
        let mut args = vec!["am", "start", "-n", component];
        for (key, value) in extras {
            args.extend(["--es", key, value]);
        }
        let (out, err) = self.shell_for(&args, APP_TIMEOUT).await?;
        // `am start` exits 0 even when the component is missing; it prints
        // `Error: ...` or `Error type N` lines instead. Warnings (an intent
        // delivered to the running activity) are success.
        let failure = out
            .lines()
            .chain(err.lines())
            .map(str::trim)
            .find(|l| l.starts_with("Error"));
        if let Some(line) = failure {
            return Err(Fault::msg(Kind::Adb, format!("am start reported: {line}")));
        }
        Ok(())
    }
}

/// What `content` printed, if anything, said plainly: a missing provider is
/// an app that is not installed, not a Java stack trace.
fn content_failure(uri: &str, out: &str) -> Option<String> {
    let out = out.trim();
    if out.is_empty() {
        return None;
    }
    if out.contains("Could not find provider") {
        let app = uri.trim_start_matches("content://");
        let app = app.split('/').next().unwrap_or(app);
        return Some(format!(
            "nothing on the phone provides {app}: is the Routedroid app installed? \
             (adb install routedroid.apk)"
        ));
    }
    Some(format!("content write reported: {out}"))
}

#[cfg(test)]
mod tests;
