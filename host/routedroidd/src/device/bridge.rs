//! The adb side of one device connection: the protocol session's id, secret
//! and nonce are generated here; the record is delivered over adb stdin
//! (§7.1), the app is launched (§7.2), and the reverse mapping is released
//! at the end.

use routedroid_proto::auth::{self, Nonce, Secret};
use routedroid_proto::bootstrap::{self, PROVIDER_URI};
use tracing::info;

use super::{DevicePorts, ReservedPort};
use crate::adb::AdbDevice;
use routedroid_ipc::fault::{Fault, FaultExt, Kind, Result};

pub const BOOTSTRAP_COMPONENT: &str = "dev.routedroid/.bootstrap.BootstrapActivity";

pub struct AdbBridge {
    adb: AdbDevice,
    ports: DevicePorts,
    port: ReservedPort,
    pub session: String,
    pub host_nonce: Nonce,
    /// Handed to the session by [`Self::bootstrap`]; `None` after that.
    secret: Option<Secret>,
}

impl AdbBridge {
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

    /// Tell the app about this session (§7.1–7.2), in two adb steps:
    ///
    /// 1. Write the bootstrap record (session id, reverse port, secret) into
    ///    the app's content provider, streamed over adb's stdin so the secret
    ///    is never part of a command line on either machine. The provider
    ///    keeps it for 60 s and only the host's own uid (shell) may write it.
    /// 2. Launch `BootstrapActivity` with the session id. The app matches it
    ///    against the stored record, connects to `127.0.0.1:<device_port>`
    ///    from the record (which adb forwards to the host's listener) and
    ///    proves the secret in the AUTH exchange. The port is not an extra:
    ///    anyone on the phone can launch the activity, so it must come from
    ///    the shell-delivered record.
    ///
    /// Nothing else happens on the phone before AUTH succeeds: no VPN prompt,
    /// no service.
    ///
    /// Returns the secret, the only copy left on the host, for the session
    /// machine (§7.3: it is consumed on AUTH). A second call fails.
    pub async fn bootstrap(&mut self) -> Result<Secret> {
        let secret = self.secret.take().ok_or_else(|| Fault::msg(Kind::Internal, "bootstrap runs once"))?;
        let record = bootstrap::encode(&self.session, self.port.device_port, &secret).expect("session id is valid hex");
        self.adb.content_write(PROVIDER_URI, record.as_slice()).await?;
        drop(record);
        info!(uri = PROVIDER_URI, "bootstrap record delivered over adb stdin");

        self.adb.am_start(BOOTSTRAP_COMPONENT, &[("session", &self.session)]).await?;
        info!(component = BOOTSTRAP_COMPONENT, "launched bootstrap activity");
        Ok(secret)
    }

    /// Undo the phone-side footprint (the reverse mapping). Never fails.
    pub async fn close(self) {
        self.ports.release(self.port).await;
    }
}
