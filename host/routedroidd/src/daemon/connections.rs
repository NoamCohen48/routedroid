//! The live device connections, keyed by serial: the authority for what this
//! daemon has created, as opposed to what adb happens to report. Owns its
//! lock, a plain mutex that no await can sit under, and owns everything a
//! connection needs to run, so starting one takes nothing but the request.

mod traffic;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use routedroid_helper_ipc::{IfName, TUN_PREFIX};
use routedroid_ipc::{ConnectionInfo, ConnectionState, Outcome, StartRequest};

use super::background::Background;
use super::devices::AttachedDevices;
use super::events::EventBus;
use super::spec::{ConnectionSpec, StartSpec};
use super::DeviceConnection;
use crate::adb::{Adb, DeviceState};
use crate::device::unusable;
use crate::fault::{Fault, Kind, Result};

type Table = HashMap<String, DeviceConnection>;

#[derive(Clone)]
pub struct DeviceConnections {
    live: Arc<Mutex<Table>>,
    devices: AttachedDevices,
    pub(super) adb: Adb,
    pub(super) helper_socket: Arc<PathBuf>,
    pub(super) events: EventBus,
    _traffic: Arc<Background>,
}

fn usage(message: String) -> Fault {
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
            events,
            _traffic: Arc::new(traffic),
        }
    }

    /// Connect one phone; returns the TUN it was given once its task runs.
    /// Progress arrives as events. Refuses a bad request, an unattached
    /// phone, or a serial, address or TUN another connection already uses,
    /// and picks a free `phoneN` if none was asked for. The checks against
    /// the table and the insert happen under one lock, so two starts cannot
    /// race.
    pub async fn start(&self, request: StartRequest) -> Result<IfName> {
        let start = StartSpec::parse(request)?;
        self.check_attached(&start.serial).await?;
        let mut live = self.lock();
        if live.contains_key(&start.serial) {
            return Err(usage(format!("{} is already connected", start.serial)));
        }
        if let Some(other) = live.values().find(|c| c.spec().phone_ip == start.phone_ip) {
            let (ip, serial) = (start.phone_ip, &other.spec().serial);
            return Err(usage(format!("{ip} is already used by {serial}")));
        }
        let taken = |name: &IfName| live.values().any(|c| &c.spec().tun == name);
        let tun = match &start.tun {
            Some(name) if taken(name) => {
                return Err(usage(format!(
                    "TUN {name} is already used by another connection"
                )))
            }
            Some(name) => name.clone(),
            None => (0..)
                .filter_map(|number| IfName::new(format!("{TUN_PREFIX}{number}")).ok())
                .find(|name| !taken(name))
                .expect("a free phoneN"),
        };
        let serial = start.serial.clone();
        let connection = DeviceConnection::spawn(self, ConnectionSpec::new(start, tun.clone()));
        live.insert(serial, connection);
        Ok(tun)
    }

    /// Ask a connection to stop and wait for its outcome. The handle stays in
    /// the table (state `Stopping`) until its task has torn everything down,
    /// so a concurrent `start` on the serial is refused.
    pub async fn stop(&self, serial: &str) -> Result<Outcome> {
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

    pub fn info(&self) -> Vec<ConnectionInfo> {
        self.lock().values().map(DeviceConnection::info).collect()
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

    /// Refuse a phone adb cannot reach before a helper session is opened. The
    /// cached list is re-read first: a phone plugged in a moment ago is a
    /// likely thing to start on.
    async fn check_attached(&self, serial: &str) -> Result<()> {
        let mut device = self.devices.get(serial);
        if device
            .as_ref()
            .is_none_or(|d| d.state != DeviceState::Device)
        {
            self.devices.refresh().await?;
            device = self.devices.get(serial);
        }
        match device {
            None => Err(usage(format!("{serial} is not attached"))),
            Some(device) if device.state == DeviceState::Device => Ok(()),
            Some(device) => {
                let reason = unusable(&device.state, serial).unwrap_or("device is not ready");
                Err(usage(format!("{serial}: {reason}")))
            }
        }
    }
}

/// A panic elsewhere must not take the table with it: every update to it is
/// a single insert or remove, so the data is consistent whatever happened.
fn lock(live: &Mutex<Table>) -> MutexGuard<'_, Table> {
    live.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
