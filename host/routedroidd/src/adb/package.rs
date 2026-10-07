//! Apps on the phone: which version of a package is installed, and
//! installing an APK.

use std::path::Path;
use std::time::Duration;

use super::AdbDevice;
use crate::fault::{Fault, Kind, Result};

/// Pushing and verifying an APK over USB 2 takes seconds; give it minutes.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);

impl AdbDevice {
    /// The versionCode of `package` on the phone, or `None` when it is not
    /// installed.
    pub async fn package_version(&self, package: &str) -> Result<Option<u64>> {
        let (out, _) = self.shell(&["dumpsys", "package", package], None).await?;
        Ok(version_code(&out, package))
    }

    /// `adb install -r APK`: install it, or replace the installed app and
    /// keep its data (the VPN consent included).
    pub async fn install(&self, apk: &Path) -> Result<()> {
        let apk = apk
            .to_str()
            .ok_or_else(|| Fault::msg(Kind::Internal, "APK path is not UTF-8"))?;
        let out = self
            .run_for(&["install", "-r", apk], INSTALL_TIMEOUT)
            .await?;
        // Older adb exits 0 when the phone refuses, and prints `Failure [...]`.
        match out
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("Failure"))
        {
            Some(line) => Err(Fault::msg(Kind::Adb, format!("adb install: {line}"))),
            None => Ok(()),
        }
    }
}

/// The first `versionCode=N` in `package`'s own section of `dumpsys package`.
/// A package that is not installed has no section (other sections, like
/// shared libraries, have version codes of their own).
fn version_code(dumpsys: &str, package: &str) -> Option<u64> {
    let header = format!("Package [{package}]");
    let mut lines = dumpsys
        .lines()
        .skip_while(|l| !l.trim().starts_with(&header));
    lines.next()?;
    lines
        .take_while(|l| !l.trim().starts_with("Package ["))
        .flat_map(str::split_whitespace)
        .find_map(|word| word.strip_prefix("versionCode=")?.parse().ok())
}

#[cfg(test)]
mod tests;
