//! The live sessions, keyed by serial. Owns its lock: callers ask for what
//! they need instead of borrowing the map, so no one can hold the lock
//! across an await.

use std::collections::HashMap;
use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind};
use routedroid_ipc::{SessionInfo, SessionState, StartRequest};
use tokio::sync::{watch, Mutex};

use super::SessionHandle;

/// What a stop needs: ask the session to stop, then watch it end.
pub type StopTarget = (Arc<watch::Sender<bool>>, watch::Receiver<SessionState>);

#[derive(Clone, Default)]
pub struct Sessions {
    live: Arc<Mutex<HashMap<String, SessionHandle>>>,
}

impl Sessions {
    /// Refuse a serial, address or TUN another session already uses, pick a
    /// free `phoneN` if none was asked for, and insert the handle `build`
    /// returns — all under one lock, so two starts cannot race.
    pub async fn start(
        &self,
        request: &StartRequest,
        build: impl FnOnce(String) -> SessionHandle,
    ) -> Result<(), Fault> {
        let mut live = self.live.lock().await;
        if live.contains_key(&request.serial) {
            return Err(Fault::msg(Kind::Usage, format!("a session is already running on {}", request.serial)));
        }
        if let Some(other) = live.values().find(|session| session.phone_ip == request.phone_ip) {
            return Err(Fault::msg(
                Kind::Usage,
                format!("{} is already used by the session on {}", request.phone_ip, other.serial),
            ));
        }
        let taken = |name: &String| live.values().any(|session| &session.tun == name);
        let tun = match &request.tun {
            Some(name) if taken(name) => {
                return Err(Fault::msg(Kind::Usage, format!("TUN {name} is already used by another session")))
            }
            Some(name) => name.clone(),
            None => (0..).map(|number| format!("phone{number}")).find(|name| !taken(name)).unwrap(),
        };
        live.insert(request.serial.clone(), build(tun));
        Ok(())
    }

    /// Drop a session's entry, but only if it is still the one that `id`
    /// names: a later session on the same serial must not be evicted.
    pub async fn remove(&self, serial: &str, id: u64) {
        let mut live = self.live.lock().await;
        if live.get(serial).is_some_and(|handle| handle.id() == id) {
            live.remove(serial);
        }
    }

    pub async fn info(&self) -> Vec<SessionInfo> {
        self.live.lock().await.values().map(SessionHandle::info).collect()
    }

    pub async fn states(&self) -> HashMap<String, SessionState> {
        self.live.lock().await.values().map(|session| (session.serial.clone(), session.state())).collect()
    }

    pub async fn stop_target(&self, serial: &str) -> Option<StopTarget> {
        let live = self.live.lock().await;
        let handle = live.get(serial)?;
        Some((handle.stop_switch(), handle.state_watch()))
    }

    /// Take every handle out; the sessions themselves keep running until
    /// they are stopped (daemon shutdown).
    pub async fn take_all(&self) -> Vec<SessionHandle> {
        self.live.lock().await.drain().map(|(_, handle)| handle).collect()
    }
}
