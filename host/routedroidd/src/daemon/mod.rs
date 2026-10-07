//! The daemon: it builds the three components the process owns and stops
//! them at the end. Nothing depends on this type — a client connection is
//! handed the components themselves — so there is no path from the control
//! socket back to the daemon.
//!
//! - [`AttachedDevices`]: what adb reports, kept current.
//! - [`DeviceConnections`]: the phones we have put on the LAN.
//! - [`EventBus`]: what clients are told about either.
//!
//! Remembered phones ([`Phones`]) are connected as they are plugged in.

mod auto;
mod background;
mod connection;
mod connections;
mod devices;
pub mod doctor;
mod events;
mod phones;
mod spec;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use crate::adb::Adb;
use crate::app::BundledApp;
pub use connection::DeviceConnection;
pub use connections::DeviceConnections;
pub use devices::{AttachedDevices, Snapshot};
pub use events::EventBus;
pub use phones::{Phones, default_path as default_phones_path};

#[derive(Clone)]
pub struct Daemon {
    devices: AttachedDevices,
    connections: DeviceConnections,
    events: EventBus,
    _auto: Arc<background::Background>,
    _notify: Option<Arc<background::Background>>,
}

impl Daemon {
    pub async fn start(
        adb: Adb,
        helper_socket: PathBuf,
        app: Option<BundledApp>,
        phones: Phones,
        notify: bool,
    ) -> Self {
        let devices = AttachedDevices::start(adb.clone()).await;
        let events = EventBus::new();
        let connections =
            DeviceConnections::new(adb, helper_socket, events.clone(), devices.clone())
                .with_app(app)
                .with_phones(phones);
        let auto = background::Background::spawn(auto::run(connections.clone(), devices.changes()));
        let notify = notify.then(|| {
            let phones = connections.phones().clone();
            background::Background::spawn(crate::notify::run(events.subscribe(), phones))
        });
        Self {
            devices,
            connections,
            events,
            _auto: Arc::new(auto),
            _notify: notify.map(Arc::new),
        }
    }

    pub fn devices(&self) -> AttachedDevices {
        self.devices.clone()
    }

    pub fn connections(&self) -> DeviceConnections {
        self.connections.clone()
    }

    pub fn events(&self) -> EventBus {
        self.events.clone()
    }

    /// Stop every connection and wait for each to end (daemon shutdown).
    pub async fn stop_all(&self) {
        self.events.publish(routedroid_ipc::Event::Shutdown);
        let mut stopping = tokio::task::JoinSet::new();
        for connection in self.connections.take_all() {
            stopping.spawn(connection.stopper().stop());
        }
        while stopping.join_next().await.is_some() {}
    }
}
