//! Secret delivery through the provider's stdin and the bootstrap launch
//! (`protocol/version-1.md` §7.1–7.2).

use tracing::info;

use super::Adb;
use crate::fault::{Fault, Kind, Result};
use routedroid_proto::bootstrap::PROVIDER_URI;

pub const BOOTSTRAP_COMPONENT: &str = "dev.routedroid/.BootstrapActivity";

impl Adb {
    /// The record (and so the secret) goes to stdin, never to an argument.
    pub async fn write_bootstrap_record(&self, record: &[u8]) -> Result<()> {
        let (stdout, stderr) = self.run_both(&["shell", "content", "write", "--uri", PROVIDER_URI], Some(record)).await?;
        // `content` exits 0 even on provider errors; it prints them (to either stream) instead.
        let out = format!("{stdout}{stderr}");
        if !out.trim().is_empty() {
            return Err(Fault::msg(Kind::Adb, format!("content write reported: {}", out.trim())));
        }
        info!(uri = PROVIDER_URI, bytes = record.len(), "bootstrap record delivered over adb stdin");
        Ok(())
    }

    pub async fn launch_bootstrap(&self, session: &str, device_port: u16) -> Result<()> {
        let port = device_port.to_string();
        let out = self
            .run(&["shell", "am", "start", "-n", BOOTSTRAP_COMPONENT, "--es", "session", session, "--ei", "device_port", &port])
            .await?;
        // `am start` exits 0 even when the component is missing; surface that.
        if out.contains("Error") || out.contains("does not exist") {
            return Err(Fault::msg(Kind::Adb, format!("am start reported: {}", out.trim())));
        }
        info!(component = BOOTSTRAP_COMPONENT, "launched bootstrap activity");
        Ok(())
    }
}
