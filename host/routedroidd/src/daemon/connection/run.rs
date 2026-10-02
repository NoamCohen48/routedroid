//! One device connection's run, from `start` accepted to everything undone.
//! Order matters for safety: host network first (so a failure leaves nothing
//! on the phone), then the reverse mapping, then the secret, then the launch.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use routedroid_ipc::{DnsChoice, EndReason, NetworkInfo, Outcome};
use tokio::sync::watch;
use tracing::{info, warn};

use super::sink::StateSink;
use crate::adb::Adb;
use crate::app_listener::AppListener;
use crate::daemon::connections::DeviceConnections;
use crate::daemon::spec::ConnectionSpec;
use crate::device::AdbBridge;
use crate::fault::{Fault, Kind, Result};
use crate::host_network::{HostNetwork, default_gateway};
use crate::session::Counters;

const HELPER_START_TIMEOUT: Duration = Duration::from_secs(15);

/// Everything one device connection needs for its whole life, in one place:
/// the connection's task owns it and drops it when the connection is over.
pub(super) struct ConnectionRun {
    adb: Adb,
    helper_socket: Arc<PathBuf>,
    pub(super) spec: Arc<ConnectionSpec>,
    pub(super) counters: Arc<Counters>,
    pub(super) sink: StateSink,
}

impl ConnectionRun {
    pub fn new(
        owner: &DeviceConnections,
        spec: Arc<ConnectionSpec>,
        counters: Arc<Counters>,
        sink: StateSink,
    ) -> Self {
        Self {
            adb: owner.adb.clone(),
            helper_socket: owner.helper_socket.clone(),
            spec,
            counters,
            sink,
        }
    }

    /// Run the connection to its end; every failure becomes a failed outcome.
    /// The sink comes back so the caller can announce the end.
    pub async fn run(self, stop_rx: watch::Receiver<bool>) -> (Outcome, StateSink) {
        let outcome = match self.connect(stop_rx).await {
            Ok(reason) => Outcome::Clean { reason },
            Err(fault) => {
                warn!(serial = %self.spec.serial, kind = %fault.kind(), "connection failed: {fault}");
                Outcome::failed(fault.kind(), fault.to_string())
            }
        };
        (outcome, self.sink)
    }

    /// Bring up the host side and the phone side, drive the connection, then
    /// undo both whatever the outcome was.
    async fn connect(&self, stop_rx: watch::Receiver<bool>) -> Result<EndReason> {
        let spec = &self.spec;
        let adb = self.adb.device(&spec.serial);
        let listener = AppListener::bind().await?;

        let start = HostNetwork::start(
            &self.helper_socket,
            &spec.lan_if,
            spec.phone_ip,
            &spec.tun,
            spec.mtu,
        );
        let mut network = tokio::time::timeout(HELPER_START_TIMEOUT, start)
            .await
            .map_err(|_| {
                let waited = HELPER_START_TIMEOUT;
                Fault::msg(
                    Kind::Helper,
                    format!("helper did not answer Start within {waited:?}"),
                )
            })??;
        let placed = self.placed(&network);
        info!(serial = adb.serial(), tun = %network.tun, host_ip = %network.host_ip,
              lan_prefix = network.lan_prefix, phone_ip = %spec.phone_ip, dns = ?placed.dns,
              helper_session = %network.session, "host network ready");
        self.sink.set_network(placed.clone());

        let mut bridge = match AdbBridge::open(adb, listener.port()).await {
            Ok(bridge) => bridge,
            Err(fault) => {
                network.stop().await;
                return Err(fault);
            }
        };
        let outcome = self
            .drive(listener, &placed, &mut bridge, &mut network, stop_rx)
            .await;
        // Concurrent: a hung adb must not delay releasing the host network.
        tokio::join!(bridge.close(), network.stop());
        outcome
    }

    /// Where the phone now is on the LAN, and the DNS it will be given.
    fn placed(&self, network: &HostNetwork) -> NetworkInfo {
        let dns = match &self.spec.dns {
            DnsChoice::Servers(servers) => servers.clone(),
            DnsChoice::None => Vec::new(),
            DnsChoice::Auto => match default_gateway(&self.spec.lan_if) {
                Some(gateway) => vec![gateway],
                None => {
                    warn!(lan_if = %self.spec.lan_if, "no default gateway on the LAN; the phone gets no DNS");
                    Vec::new()
                }
            },
        };
        NetworkInfo {
            phone_ip: self.spec.phone_ip,
            host_ip: network.host_ip,
            lan_prefix: network.lan_prefix,
            dns,
            lease: None,
        }
    }
}
