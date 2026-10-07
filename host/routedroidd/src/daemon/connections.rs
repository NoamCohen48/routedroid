//! The live device connections, keyed by serial: the authority for what this
//! daemon has created, as opposed to what adb happens to report. Owns its
//! lock, a plain mutex that no await can sit under, and owns everything a
//! connection needs to run, so starting one takes nothing but the request.

mod admit;
mod resolve;
mod start;
mod traffic;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use routedroid_helper_ipc::IfName;
use routedroid_ipc::{ConnectionInfo, ConnectionState, InterfaceInfo, Outcome};

use super::DeviceConnection;
use super::background::Background;
use super::devices::AttachedDevices;
use super::events::EventBus;
use super::phones::Phones;
use crate::adb::Adb;
use crate::app::BundledApp;
use crate::fault::{Fault, Kind, Result};
use crate::host_network;

type Table = HashMap<String, DeviceConnection>;

/// A start, accepted: the phone, LAN and TUN it was given.
#[derive(Debug)]
pub struct Accepted {
    pub serial: String,
    pub lan_if: String,
    /// `None`: leasing one.
    pub phone_ip: Option<std::net::Ipv4Addr>,
    pub tun: IfName,
}

#[derive(Clone)]
pub struct DeviceConnections {
    live: Arc<Mutex<Table>>,
    pub(super) devices: AttachedDevices,
    pub(super) adb: Adb,
    pub(super) helper_socket: Arc<PathBuf>,
    /// Installed on phones that need it before they connect, when there is one.
    pub(super) app: Option<BundledApp>,
    pub(super) events: EventBus,
    pub(super) phones: Phones,
    _traffic: Arc<Background>,
}

pub(super) fn usage(message: String) -> Fault {
    Fault::msg(Kind::Usage, message)
}

impl DeviceConnections {
    pub fn new(
        adb: Adb,
        helper_socket: PathBuf,
        events: EventBus,
        devices: AttachedDevices,
    ) -> Self {
        let live = Arc::<Mutex<Table>>::default();
        let traffic = Background::spawn(traffic::ticker(live.clone(), events.clone()));
        Self {
            live,
            devices,
            adb,
            helper_socket: Arc::new(helper_socket),
            app: None,
            events,
            phones: Phones::default(),
            _traffic: Arc::new(traffic),
        }
    }

    /// Start remembered phones with what was remembered for them.
    pub fn with_phones(mut self, phones: Phones) -> Self {
        self.phones = phones;
        self
    }

    pub fn phones(&self) -> &Phones {
        &self.phones
    }

    /// Carry `app` to the phones that need it.
    pub fn with_app(mut self, app: Option<BundledApp>) -> Self {
        self.app = app;
        self
    }

    /// Ask a connection to stop and wait for its outcome. The handle stays in
    /// the table (state `Stopping`) until its task has torn everything down,
    /// so a concurrent `start` on the serial is refused.
    pub async fn stop(&self, serial: &str) -> Result<Outcome> {
        let serial = &self.phones.serial(serial);
        let stopper = self
            .lock()
            .get(serial)
            .map(DeviceConnection::stopper)
            .ok_or_else(|| usage(format!("{serial} is not connected")))?;
        stopper.stop().await
    }

    /// Drop a connection's entry, but only if it is still the one that `id`
    /// names: a later connection on the same serial must not be evicted.
    pub fn remove(&self, serial: &str, id: u64) {
        let mut live = self.lock();
        if live.get(serial).is_some_and(|handle| handle.id() == id) {
            live.remove(serial);
        }
    }

    pub fn helper_socket(&self) -> &std::path::Path {
        &self.helper_socket
    }

    /// The helper's word on which links may carry phones.
    pub async fn interfaces(&self) -> Result<Vec<InterfaceInfo>> {
        host_network::interfaces(&self.helper_socket).await
    }

    /// Every connection, with its phone's remembered name.
    pub fn info(&self) -> Vec<ConnectionInfo> {
        let phones = self.phones.all();
        let named = |mut info: ConnectionInfo| {
            info.name = phones
                .iter()
                .find(|p| p.serial == info.serial)
                .and_then(|p| p.name.clone());
            info
        };
        self.lock()
            .values()
            .map(DeviceConnection::info)
            .map(named)
            .collect()
    }

    pub fn events(&self) -> &EventBus {
        &self.events
    }

    pub fn states(&self) -> HashMap<String, ConnectionState> {
        let live = self.lock();
        live.iter()
            .map(|(serial, c)| (serial.clone(), c.state()))
            .collect()
    }

    /// Take every handle out; the connections themselves keep running until
    /// they are stopped (daemon shutdown).
    pub fn take_all(&self) -> Vec<DeviceConnection> {
        self.lock().drain().map(|(_, handle)| handle).collect()
    }

    fn lock(&self) -> MutexGuard<'_, Table> {
        lock(&self.live)
    }
}

/// A panic elsewhere must not take the table with it: every update to it is
/// a single insert or remove, so the data is consistent whatever happened.
fn lock(live: &Mutex<Table>) -> MutexGuard<'_, Table> {
    live.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
