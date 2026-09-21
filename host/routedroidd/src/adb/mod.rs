//! Thin binding to the `adb` executable with a timeout on every command.
//! Knows adb's commands and output formats and nothing about Routedroid's
//! protocol or its transport policy (that is `device::*`).
//!
//! [`Adb`] is the executable (server-level commands); [`AdbDevice`] is a
//! handle to one serial, and every per-device command goes through it so
//! `-s` can never be forgotten when several phones are attached. The adb
//! command lines stay inside this module: callers get one method per
//! operation (`reverse_add`, `content_write`, `am_start`, ...).

mod android;
mod device;
mod devices;
mod reverse;

pub use android::Extra;
pub use device::AdbDevice;
pub use devices::{parse_devices, Device, DeviceState};
#[cfg(test)]
pub use reverse::parse_reverse_list;
pub use reverse::ReverseMapping;

use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use tokio::process::Command;

use routedroid_ipc::fault::{Fault, FaultExt, Kind, Result};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Adb {
    binary: String,
    timeout: Duration,
}

impl Adb {
    pub fn new(binary: &str, timeout: Duration) -> Self {
        Self { binary: binary.to_string(), timeout }
    }

    /// A handle for `adb -s SERIAL ...`. Cheap; nothing runs until a command does.
    pub fn device(&self, serial: &str) -> AdbDevice {
        AdbDevice::new(self.clone(), serial)
    }

    /// `adb devices -l`: every device the server knows about.
    pub async fn devices(&self) -> Result<Vec<Device>> {
        let mut c = Command::new(&self.binary);
        c.args(["devices", "-l"]).stdin(Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(self.timeout, c.output())
            .await
            .map_err(|_| Fault::msg(Kind::Adb, "adb devices timed out"))?
            .context("spawn adb devices")
            .fault(Kind::Adb)?;
        if !out.status.success() {
            return Err(Fault::msg(
                Kind::Adb,
                format!("adb devices failed: {}", String::from_utf8_lossy(&out.stderr).trim()),
            ));
        }
        Ok(parse_devices(&String::from_utf8_lossy(&out.stdout)))
    }
}
