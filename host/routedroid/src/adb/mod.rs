//! ADB wrapper pinned to one serial, with a timeout on every command.
//!
//! Only the USB transport is accepted in version 1 (`protocol/version-1.md`
//! §1); [`serial::Transport`] classifies serials before anything runs.

mod bootstrap;
mod devices;
mod reverse;
mod serial;

pub use bootstrap::{BOOTSTRAP_COMPONENT, PACKAGE};
pub use devices::{parse_devices, Device, DeviceState};
pub use reverse::{list_has_exactly, parse_reverse_list};
pub use serial::Transport;

use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context};
use tokio::process::Command;

use crate::fault::{Fault, FaultExt, Kind, Result};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Adb {
    binary: String,
    serial: String,
    timeout: Duration,
}

impl Adb {
    /// Refuses network serials (§1) before the first command runs.
    pub fn new(binary: &str, serial: &str, timeout: Duration) -> Result<Self> {
        match Transport::classify(serial) {
            Transport::Usb | Transport::Emulator => Ok(Self { binary: binary.to_string(), serial: serial.to_string(), timeout }),
            Transport::Network => Err(Fault::msg(
                Kind::Transport,
                format!("serial {serial:?} is a network transport; version 1 supports USB ADB only"),
            )),
            Transport::Invalid => Err(Fault::msg(Kind::Usage, format!("serial {serial:?} is not a valid ADB serial"))),
        }
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.binary);
        c.arg("-s").arg(&self.serial).args(args).stdin(Stdio::null()).kill_on_drop(true);
        c
    }

    /// Run `adb -s SERIAL <args>`; stdout on success, or an error carrying
    /// both streams. Never passes secrets: callers use [`Self::run_with_stdin`].
    pub async fn run(&self, args: &[&str]) -> Result<String> {
        self.run_with_stdin(args, None).await
    }

    pub async fn run_with_stdin(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<String> {
        let mut cmd = self.command(args);
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let desc = format!("adb -s {} {}", self.serial, args.join(" "));
        let fut = async {
            let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().with_context(|| format!("spawn {desc}"))?;
            if let Some(bytes) = stdin {
                use tokio::io::AsyncWriteExt;
                let mut pipe = child.stdin.take().expect("piped stdin");
                pipe.write_all(bytes).await.context("write to adb stdin")?;
                drop(pipe);
            }
            let out = child.wait_with_output().await.with_context(|| format!("wait {desc}"))?;
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            if !out.status.success() {
                bail!("{desc} failed ({}): {}{}", out.status, stdout.trim(), stderr.trim());
            }
            Ok::<_, anyhow::Error>(stdout)
        };
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(r) => r.fault(Kind::Adb),
            Err(_) => Err(Fault::msg(Kind::Adb, format!("{desc} timed out after {:?}", self.timeout))),
        }
    }

    /// `adb devices -l` is not serial-scoped; it is used to find and check a device.
    pub async fn devices(binary: &str, timeout: Duration) -> Result<Vec<Device>> {
        let mut c = Command::new(binary);
        c.args(["devices", "-l"]).stdin(Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(timeout, c.output())
            .await
            .map_err(|_| Fault::msg(Kind::Adb, "adb devices timed out"))?
            .context("spawn adb devices")
            .fault(Kind::Adb)?;
        if !out.status.success() {
            return Err(Fault::msg(Kind::Adb, format!("adb devices failed: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(parse_devices(&String::from_utf8_lossy(&out.stdout)))
    }
}
