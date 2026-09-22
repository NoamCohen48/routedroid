//! The live device connections, keyed by serial. Owns its lock: callers ask
//! for what they need instead of borrowing the map, so no one can hold the
//! lock across an await. A connection stays here until its own task has
//! finished tearing it down, whatever clients come and go meanwhile.

use std::collections::HashMap;
use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind};
use routedroid_ipc::{ConnectionInfo, ConnectionState, StartRequest};
use tokio::sync::{watch, Mutex};

use super::DeviceConnection;

/// What a stop needs: ask the connection to stop, then watch it end.
pub type StopTarget = (Arc<watch::Sender<bool>>, watch::Receiver<ConnectionState>);

#[derive(Clone, Default)]
pub struct DeviceConnections {
    live: Arc<Mutex<HashMap<String, DeviceConnection>>>,
}

impl DeviceConnections {
    /// Refuse a serial, address or TUN another connection already uses, pick a
    /// free `phoneN` if none was asked for, and insert the handle `build`
    /// returns — all under one lock, so two starts cannot race.
    pub async fn start(
        &self,
        request: &StartRequest,
        build: impl FnOnce(String) -> DeviceConnection,
    ) -> Result<(), Fault> {
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
        live.insert(request.serial.clone(), build(tun));
        Ok(())
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
        self.live.lock().await.values().map(DeviceConnection::info).collect()
    }

    pub async fn states(&self) -> HashMap<String, ConnectionState> {
        self.live.lock().await.values().map(|connection| (connection.serial.clone(), connection.state())).collect()
    }

    pub async fn stop_target(&self, serial: &str) -> Option<StopTarget> {
        let live = self.live.lock().await;
        let handle = live.get(serial)?;
        Some((handle.stop_switch(), handle.state_watch()))
    }

    /// Take every handle out; the connections themselves keep running until
    /// they are stopped (daemon shutdown).
    pub async fn take_all(&self) -> Vec<DeviceConnection> {
        self.live.lock().await.drain().map(|(_, handle)| handle).collect()
    }
}
