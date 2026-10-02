//! The live device connections, keyed by serial: the authority for what this
//! daemon has created, as opposed to what adb happens to report. Owns its
//! lock — callers ask for what they need instead of borrowing the map, so no
//! one can hold the lock across an await — and owns everything a connection
//! needs to run, so starting one takes nothing but the request.

mod traffic;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind, Result};
use routedroid_ipc::{ConnectionInfo, ConnectionState, StartRequest};
use routedroid_proto::messages::is_unicast_host;
use tokio::sync::Mutex;

use super::background::Background;
use super::devices::AttachedDevices;
use super::events::EventBus;
use super::DeviceConnection;
use crate::adb::{Adb, DeviceState};
use crate::device::{unusable, Transport};

pub(super) type Live = Arc<Mutex<HashMap<String, DeviceConnection>>>;

#[derive(Clone)]
pub struct DeviceConnections {
    live: Live,
    devices: AttachedDevices,
    pub(super) adb: Adb,
    pub(super) helper_socket: Arc<PathBuf>,
    pub(super) events: EventBus,
    _traffic: Arc<Background>,
}

/// The addresses go into CONFIGURE_VPN, which the app refuses unless they
/// are unicast host addresses (§4.4); refuse them here, before anything runs.
fn check_addresses(request: &StartRequest) -> Result<()> {
    if !is_unicast_host(request.phone_ip) {
        return Err(Fault::msg(Kind::Usage, format!("{} is not a unicast host address", request.phone_ip)));
    }
    if let Some(dns) = request.dns.iter().find(|dns| !is_unicast_host(**dns)) {
        return Err(Fault::msg(Kind::Usage, format!("DNS server {dns} is not a unicast host address")));
    }
    Ok(())
}

impl DeviceConnections {
    pub fn new(adb: Adb, helper_socket: PathBuf, events: EventBus, devices: AttachedDevices) -> Self {
        let live = Live::default();
        let traffic = Background::spawn(traffic::ticker(live.clone(), events.clone()));
        Self { live, devices, adb, helper_socket: Arc::new(helper_socket), events, _traffic: Arc::new(traffic) }
    }

    /// Connect one phone. Returns once its task is running; progress arrives
    /// as events. Refuses a serial, address or TUN another connection already
    /// uses, and picks a free `phoneN` if none was asked for — all under one
    /// lock, so two starts cannot race.
    pub async fn start(&self, request: StartRequest) -> Result<()> {
        Transport::check(&request.serial, request.allow_network_adb)?;
        check_addresses(&request)?;
        self.check_attached(&request.serial).await?;
        let mut live = self.live.lock().await;
        if live.contains_key(&request.serial) {
            return Err(Fault::msg(Kind::Usage, format!("{} is already connected", request.serial)));
        }
        if let Some(other) = live.values().find(|connection| connection.phone_ip == request.phone_ip) {
            return Err(Fault::msg(Kind::Usage, format!("{} is already used by {}", request.phone_ip, other.serial)));
        }
        let taken = |name: &String| live.values().any(|connection| &connection.tun == name);
        let tun = match &request.tun {
            Some(name) if taken(name) => {
                return Err(Fault::msg(Kind::Usage, format!("TUN {name} is already used by another connection")))
            }
            Some(name) => name.clone(),
            None => (0..).map(|number| format!("phone{number}")).find(|name| !taken(name)).unwrap(),
        };
        let serial = request.serial.clone();
        live.insert(serial, DeviceConnection::spawn(self, request, tun));
        Ok(())
    }

    /// Ask a connection to stop and wait for its outcome. The handle stays in
    /// the table (state `Stopping`) until its task has torn everything down,
    /// so a concurrent `start` on the serial is refused.
    pub async fn stop(&self, serial: &str) -> Result<()> {
        let (stop, state) = {
            let live = self.live.lock().await;
            let handle =
                live.get(serial).ok_or_else(|| Fault::msg(Kind::Usage, format!("{serial} is not connected")))?;
            (handle.stop_switch(), handle.state_watch())
        };
        match DeviceConnection::stop_and_wait_on(&stop, state).await {
            Some(outcome) if outcome.ok => Ok(()),
            Some(outcome) => Err(Fault::msg(outcome.kind.unwrap_or(Kind::Internal), outcome.message)),
            None => {
                Err(Fault::msg(Kind::Internal, format!("{serial} is still disconnecting; watch for its ended event")))
            }
        }
    }

    /// Drop a connection's entry, but only if it is still the one that `id`
    /// names: a later connection on the same serial must not be evicted.
    pub async fn remove(&self, serial: &str, id: u64) {
        let mut live = self.live.lock().await;
        if live.get(serial).is_some_and(|handle| handle.id() == id) {
            live.remove(serial);
        }
    }

    pub async fn info(&self) -> Vec<ConnectionInfo> {
        Self::snapshot(&self.live).await
    }

    pub async fn states(&self) -> HashMap<String, ConnectionState> {
        self.live.lock().await.values().map(|connection| (connection.serial.clone(), connection.state())).collect()
    }

    /// Take every handle out; the connections themselves keep running until
    /// they are stopped (daemon shutdown).
    pub async fn take_all(&self) -> Vec<DeviceConnection> {
        self.live.lock().await.drain().map(|(_, handle)| handle).collect()
    }

    pub(super) async fn snapshot(live: &Live) -> Vec<ConnectionInfo> {
        live.lock().await.values().map(DeviceConnection::info).collect()
    }

    /// Refuse a phone adb cannot reach before a helper session is opened. The
    /// cached list is re-read first: a phone plugged in a moment ago is a
    /// likely thing to start on.
    async fn check_attached(&self, serial: &str) -> Result<()> {
        let mut device = self.devices.get(serial);
        if device.as_ref().is_none_or(|device| device.state != DeviceState::Device) {
            self.devices.refresh().await?;
            device = self.devices.get(serial);
        }
        match device {
            None => Err(Fault::msg(Kind::Usage, format!("{serial} is not attached"))),
            Some(device) if device.state == DeviceState::Device => Ok(()),
            Some(device) => {
                let reason = unusable(&device.state, serial).unwrap_or("device is not ready");
                Err(Fault::msg(Kind::Usage, format!("{serial}: {reason}")))
            }
        }
    }
}
