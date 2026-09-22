//! Daemon state: the adb binding, live sessions by serial, and the event
//! bus every subscribed connection listens to. Everything outside this
//! module goes through `Daemon`'s methods; the fields are private and only
//! this module's own files (`api`, `devices`, `run`, `session`) touch them.

mod api;
mod devices;
mod run;
mod session;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use routedroid_ipc::Event;
use tokio::sync::{broadcast, Mutex, MutexGuard};

use crate::adb::Adb;
pub use session::SessionHandle;

pub struct Daemon {
    adb: Adb,
    helper_socket: PathBuf,
    sessions: Mutex<HashMap<String, SessionHandle>>,
    events: broadcast::Sender<Event>,
}

impl Daemon {
    pub fn new(adb: Adb, helper_socket: PathBuf) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self { adb, helper_socket, sessions: Mutex::new(HashMap::new()), events })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn publish(&self, event: Event) {
        // No subscribers is not an error.
        let _ = self.events.send(event);
    }

    /// The live sessions. Private on purpose: callers outside this module
    /// ask for what they need (`status`, `start`, `stop`) instead of
    /// reaching into the map — and cannot hold the lock across an await.
    async fn sessions(&self) -> MutexGuard<'_, HashMap<String, SessionHandle>> {
        self.sessions.lock().await
    }

    /// Stop every session and wait for each to end (daemon shutdown).
    pub async fn stop_all(&self) {
        self.publish(Event::Shutdown);
        let handles: Vec<SessionHandle> = self.sessions().await.drain().map(|(_, handle)| handle).collect();
        let mut stopping = tokio::task::JoinSet::new();
        for handle in handles {
            stopping.spawn(async move { handle.stop_and_wait().await });
        }
        while stopping.join_next().await.is_some() {}
    }
}
