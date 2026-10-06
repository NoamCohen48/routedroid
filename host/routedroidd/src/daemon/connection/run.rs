//! One device connection's run, from `start` accepted to everything undone.
//! Order matters for safety: host network first (so a failure leaves nothing
//! on the phone), then the reverse mapping, then the secret, then the launch.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use routedroid_helper_ipc::DeviceId;
use routedroid_ipc::{ConnectionState, DnsChoice, EndReason, Lease, NetworkInfo, Outcome};
use tokio::sync::watch;
use tracing::{info, warn};

use super::sink::StateSink;
use crate::adb::Adb;
use crate::daemon::connections::DeviceConnections;
use crate::daemon::devices::AttachedDevices;
use crate::daemon::spec::ConnectionSpec;
use crate::fault::{Fault, Kind, Result};
use crate::host_network::{HostNetwork, default_gateway};
use crate::session::Counters;

/// The helper may spend up to 30 s on a lease, ARP probes included.
const HELPER_START_TIMEOUT: Duration = Duration::from_secs(45);

/// Everything one device connection needs for its whole life, in one place:
/// the connection's task owns it and drops it when the connection is over.
pub(super) struct ConnectionRun {
    pub(super) adb: Adb,
    pub(super) devices: AttachedDevices,
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
            devices: owner.devices.clone(),
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

    /// Bring up the host side, run the phone's sessions on it, then undo
    /// both whatever the outcome was.
    async fn connect(&self, mut stop_rx: watch::Receiver<bool>) -> Result<EndReason> {
        let spec = &self.spec;

        let start = HostNetwork::start(
            &self.helper_socket,
            &spec.lan_if,
            spec.phone_ip,
            DeviceId::from_serial(&spec.serial),
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
        let mut placed = self.placed(&network);
        info!(serial = %spec.serial, tun = %network.tun, host_ip = %network.host_ip,
              lan_prefix = network.lan_prefix, phone_ip = %network.phone_ip, dns = ?placed.dns,
              helper_session = %network.session, "host network ready");
        self.sink.set_network(placed.clone());

        let mut events = network.listen();
        let (outcome, bridge) = self
            .sessions(&mut network, &mut placed, &mut events, &mut stop_rx)
            .await;
        self.sink.set(ConnectionState::Stopping);
        // Concurrent: a hung adb must not delay releasing the host network.
        let close = async {
            if let Some(bridge) = bridge {
                bridge.close().await;
            }
        };
        tokio::join!(close, network.stop());
        outcome
    }

    /// Where the phone now is on the LAN, and the DNS it will be given:
    /// with `auto`, the lease's servers, else the LAN's default gateway.
    fn placed(&self, network: &HostNetwork) -> NetworkInfo {
        let leased_dns = network.lease.as_ref().map(|l| l.dns.clone());
        let dns = match &self.spec.dns {
            DnsChoice::Servers(servers) => servers.clone(),
            DnsChoice::None => Vec::new(),
            DnsChoice::Auto if leased_dns.as_ref().is_some_and(|d| !d.is_empty()) => {
                leased_dns.unwrap_or_default()
            }
            DnsChoice::Auto => match default_gateway(&self.spec.lan_if) {
                Some(gateway) => vec![gateway],
                None => {
                    warn!(lan_if = %self.spec.lan_if, "no default gateway on the LAN; the phone gets no DNS");
                    Vec::new()
                }
            },
        };
        NetworkInfo {
            phone_ip: network.phone_ip,
            host_ip: network.host_ip,
            lan_prefix: network.lan_prefix,
            dns,
            lease: network.lease.as_ref().map(lease),
        }
    }
}

/// The lease as clients see it.
pub(super) fn lease(lease: &routedroid_helper_ipc::Lease) -> Lease {
    Lease {
        server: lease.server,
        expires_at: lease.expires_at,
    }
}
