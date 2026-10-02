//! One device connection's run, from `start` accepted to everything undone.
//! Order matters for safety: host network first (so a failure leaves nothing
//! on the phone), then the reverse mapping, then the secret, then the launch.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::fault::{Fault, Kind, Result};
use routedroid_ipc::{Outcome, StartRequest};
use routedroid_proto::frame::DEFAULT_MTU;
use tokio::sync::watch;
use tracing::info;

use super::StateSink;
use crate::adb::Adb;
use crate::app_listener::AppListener;
use crate::device::{AdbBridge, Transport};
use crate::host_network::HostNetwork;
use crate::session::Counters;

const HELPER_START_TIMEOUT: Duration = Duration::from_secs(15);

/// Everything one device connection needs for its whole life, in one place:
/// the connection's task owns it and drops it when the connection is over.
pub(crate) struct ConnectionRun<'a> {
    adb: Adb,
    helper_socket: Arc<PathBuf>,
    pub(super) request: StartRequest,
    tun: String,
    pub(super) counters: Arc<Counters>,
    pub(super) sink: &'a StateSink,
}

impl<'a> ConnectionRun<'a> {
    pub fn new(
        adb: Adb,
        helper_socket: Arc<PathBuf>,
        request: StartRequest,
        tun: String,
        counters: Arc<Counters>,
        sink: &'a StateSink,
    ) -> Self {
        Self {
            adb,
            helper_socket,
            request,
            tun,
            counters,
            sink,
        }
    }

    /// Run the connection to its end; every failure becomes a failed outcome.
    pub async fn run(self, stop_rx: watch::Receiver<bool>) -> Outcome {
        match self.connect(stop_rx).await {
            Ok(message) => Outcome {
                ok: true,
                kind: None,
                message: message.into(),
            },
            Err(fault) => {
                tracing::warn!(kind = fault.kind().as_str(), "connection failed: {fault}");
                Outcome {
                    ok: false,
                    kind: Some(fault.kind()),
                    message: fault.to_string(),
                }
            }
        }
    }

    fn mtu(&self) -> Result<u32> {
        let mtu = self.request.mtu.unwrap_or(DEFAULT_MTU);
        if !(576..=65535).contains(&mtu) {
            return Err(Fault::msg(
                Kind::Usage,
                format!("mtu {mtu} outside 576..=65535"),
            ));
        }
        Ok(mtu)
    }

    /// Bring up the host side and the phone side, drive the connection, then
    /// undo both whatever the outcome was.
    async fn connect(&self, stop_rx: watch::Receiver<bool>) -> Result<&'static str> {
        Transport::check(&self.request.serial, self.request.allow_network_adb)?;
        let mtu = self.mtu()?;
        let adb = self.adb.device(&self.request.serial);
        let listener = AppListener::bind().await?;

        let mut network = tokio::time::timeout(
            HELPER_START_TIMEOUT,
            HostNetwork::start(
                &self.helper_socket,
                &self.request.lan_if,
                self.request.phone_ip,
                &self.tun,
                mtu,
            ),
        )
        .await
        .map_err(|_| {
            Fault::msg(
                Kind::Helper,
                format!("helper did not answer Start within {HELPER_START_TIMEOUT:?}"),
            )
        })??;
        info!(serial = adb.serial(), tun = %network.tun, host_ip = %network.host_ip, lan_prefix = network.lan_prefix,
              phone_ip = %self.request.phone_ip, helper_session = %network.session, "host network ready");

        let mut bridge = match AdbBridge::open(adb, listener.port()).await {
            Ok(bridge) => bridge,
            Err(fault) => {
                network.stop().await;
                return Err(fault);
            }
        };
        let outcome = self
            .drive(mtu, listener, &mut bridge, &mut network, stop_rx)
            .await;
        // Concurrent: a hung adb must not delay releasing the host network.
        tokio::join!(bridge.close(), network.stop());
        outcome
    }
}
