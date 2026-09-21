//! One session's footprint on the phone: session id, secret and nonce are
//! generated here; the record is delivered over adb stdin (§7.1), the app is
//! launched (§7.2), and the reverse mapping is released at the end.

use routedroid_proto::auth::{self, Nonce, Secret};
use routedroid_proto::bootstrap::{self, PROVIDER_URI};
use tracing::info;

use super::{DevicePorts, ReservedPort};
use crate::adb::AdbDevice;
use crate::fault::{Fault, FaultExt, Kind, Result};

pub const BOOTSTRAP_COMPONENT: &str = "dev.routedroid/.BootstrapActivity";

pub struct DeviceSession {
    adb: AdbDevice,
    ports: DevicePorts,
    port: ReservedPort,
    pub session: String,
    pub host_nonce: Nonce,
    /// Taken by the session machine; `None` once handed over.
    secret: Option<Secret>,
}

impl DeviceSession {
    /// Reserve the reverse port and generate this session's credentials.
    /// Nothing has been sent to the phone yet.
    pub async fn open(adb: AdbDevice, host_port: u16) -> Result<Self> {
        let seed = u16::from_le_bytes(auth::random_bytes::<2>().fault(Kind::Internal)?);
        let ports = DevicePorts::new(adb.clone());
        let port = ports.reserve(host_port, seed).await?;
        Ok(Self {
            adb,
            ports,
            port,
            session: hex::encode(auth::random_bytes::<8>().fault(Kind::Internal)?),
            host_nonce: auth::random_nonce().fault(Kind::Internal)?,
            secret: Some(Secret::random().fault(Kind::Internal)?),
        })
    }

    pub fn device_port(&self) -> u16 {
        self.port.device_port
    }

    /// The single copy of the secret, for the session machine (§7.3: it is
    /// consumed on AUTH). Panics if called twice.
    pub fn take_secret(&mut self) -> Secret {
        self.secret.take().expect("secret taken once")
    }

    /// Stream the record to the provider's stdin, then launch the activity.
    pub async fn bootstrap(&self) -> Result<()> {
        let secret = self.secret.as_ref().expect("bootstrap before the secret is handed over");
        let record = bootstrap::encode(&self.session, secret).expect("session id is valid hex");
        let (stdout, stderr) =
            self.adb.shell(&["content", "write", "--uri", PROVIDER_URI], Some(record.as_slice())).await?;
        drop(record);
        // `content` exits 0 even on provider errors; it prints them (to either stream) instead.
        let out = format!("{stdout}{stderr}");
        if !out.trim().is_empty() {
            return Err(Fault::msg(Kind::Adb, format!("content write reported: {}", out.trim())));
        }
        info!(uri = PROVIDER_URI, "bootstrap record delivered over adb stdin");

        let port = self.port.device_port.to_string();
        let (out, _) = self
            .adb
            .shell(
                &[
                    "am",
                    "start",
                    "-n",
                    BOOTSTRAP_COMPONENT,
                    "--es",
                    "session",
                    &self.session,
                    "--ei",
                    "device_port",
                    &port,
                ],
                None,
            )
            .await?;
        // `am start` exits 0 even when the component is missing; surface that.
        if out.contains("Error") || out.contains("does not exist") {
            return Err(Fault::msg(Kind::Adb, format!("am start reported: {}", out.trim())));
        }
        info!(component = BOOTSTRAP_COMPONENT, "launched bootstrap activity");
        Ok(())
    }

    /// Undo the phone-side footprint (the reverse mapping). Never fails.
    pub async fn close(self) {
        self.ports.release(self.port).await;
    }
}
