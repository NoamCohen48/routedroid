//! The Routedroid app this daemon carries, embedded at build time (see
//! build.rs), and putting it on a phone that needs it: one without the app,
//! or with an older version. A newer app on the phone is left alone.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{info, warn};

use crate::adb::AdbDevice;
use crate::fault::{Fault, FaultExt, Kind, Result};

pub const PACKAGE: &str = "dev.routedroid";

static EMBEDDED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.apk"));

#[derive(Debug, Clone, Copy)]
pub struct BundledApp {
    apk: &'static [u8],
    version_code: u64,
}

/// What a phone needs before it can connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Nothing,
    Install,
    Upgrade { from: u64 },
}

impl BundledApp {
    /// The app built into this binary, if one was.
    pub fn embedded() -> Option<Self> {
        let version = version_code(env!("CARGO_PKG_VERSION")).expect("version is X.Y.Z");
        (!EMBEDDED.is_empty()).then_some(Self::new(EMBEDDED, version))
    }

    pub fn new(apk: &'static [u8], version_code: u64) -> Self {
        Self { apk, version_code }
    }

    pub fn version(&self) -> String {
        version_name(self.version_code)
    }

    pub fn need(&self, installed: Option<u64>) -> Need {
        match installed {
            None => Need::Install,
            Some(from) if from < self.version_code => Need::Upgrade { from },
            Some(_) => Need::Nothing,
        }
    }

    /// Install or upgrade the app on `phone` if it needs it; `installing` is
    /// called first when it does. A failed upgrade is only a warning: the
    /// installed app may still speak the protocol (the handshake decides).
    /// Returns whether it tried.
    pub async fn ensure(&self, phone: &AdbDevice, installing: impl FnOnce()) -> Result<bool> {
        let need = self.need(phone.package_version(PACKAGE).await?);
        if need == Need::Nothing {
            return Ok(false);
        }
        installing();
        info!(serial = %phone.serial(), ?need, version = %self.version(), "installing the app");
        let apk = self.stage(phone.serial()).await?;
        let installed = phone.install(&apk).await;
        let _ = tokio::fs::remove_file(&apk).await;
        match (installed, need) {
            (Ok(()), _) => Ok(true),
            (Err(fault), Need::Upgrade { from }) => {
                let from = version_name(from);
                warn!(serial = %phone.serial(), "could not upgrade the app from {from}: {fault}");
                Ok(true)
            }
            (Err(fault), _) => Err(Fault::msg(
                Kind::Adb,
                format!("could not install the Routedroid app on the phone: {fault}"),
            )),
        }
    }

    /// `adb install` takes a file: write the APK to the runtime directory.
    async fn stage(&self, serial: &str) -> Result<PathBuf> {
        let dir =
            std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
        let serial: String = serial
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = dir.join(format!("routedroid-app-{pid}-{n}-{serial}.apk"));
        tokio::fs::write(&path, self.apk)
            .await
            .fault(Kind::Internal)?;
        Ok(path)
    }
}

/// The app's versionCode for a product version, as android/app/build.gradle.kts
/// computes it from the same `X.Y.Z`.
pub fn version_code(version: &str) -> Option<u64> {
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    let (major, minor, patch) = (parts.next()??, parts.next()??, parts.next()??);
    (parts.next().is_none() && minor < 1_000 && patch < 1_000)
        .then_some(major * 1_000_000 + minor * 1_000 + patch)
}

fn version_name(code: u64) -> String {
    format!(
        "{}.{}.{}",
        code / 1_000_000,
        code / 1_000 % 1_000,
        code % 1_000
    )
}

#[cfg(test)]
mod tests;
