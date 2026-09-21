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

    /// `am start -n COMPONENT` with string (`--es`) and integer (`--ei`) extras.
    pub async fn am_start(&self, component: &str, extras: &[Extra<'_>]) -> Result<()> {
        let mut args = vec!["am", "start", "-n", component];
        let ints: Vec<String> = extras.iter().filter_map(|e| e.int_value()).collect();
        let mut ints = ints.iter();
        for extra in extras {
            match extra {
                Extra::Str(key, value) => args.extend(["--es", key, value]),
                Extra::Int(key, _) => args.extend(["--ei", key, ints.next().expect("one string per Int extra")]),
            }
        }
        let (out, _) = self.shell(&args, None).await?;
        // `am start` exits 0 even when the component is missing; surface that.
        if out.contains("Error") || out.contains("does not exist") {
            return Err(Fault::msg(Kind::Adb, format!("am start reported: {}", out.trim())));
        }
        Ok(())
    }
}

/// An intent extra for [`AdbDevice::am_start`].
#[derive(Debug, Clone, Copy)]
pub enum Extra<'a> {
    Str(&'a str, &'a str),
    Int(&'a str, i64),
}

impl Extra<'_> {
    fn int_value(&self) -> Option<String> {
        match self {
            Self::Int(_, v) => Some(v.to_string()),
            Self::Str(..) => None,
        }
    }
}
