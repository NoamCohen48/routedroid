//! Daemon state: the adb binding, live sessions by serial, and the event
//! bus every subscribed connection listens to.

mod api;
mod devices;
mod run;
mod session;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use routedroid_ipc::Event;
use tokio::sync::{broadcast, Mutex};

use crate::adb::Adb;
pub use api::handle;
pub use devices::{traffic_ticker, watch_devices};
pub use session::SessionHandle;

pub struct Daemon {
    pub adb: Adb,
    pub helper_socket: PathBuf,
    pub sessions: Mutex<HashMap<String, SessionHandle>>,
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

    /// Stop every session and wait for each to end (daemon shutdown).
    pub async fn stop_all(&self) {
        self.publish(Event::Shutdown);
        let handles: Vec<SessionHandle> = self.sessions.lock().await.drain().map(|(_, handle)| handle).collect();
        let mut stopping = tokio::task::JoinSet::new();
        for handle in handles {
            stopping.spawn(async move { handle.stop_and_wait().await });
        }
        while stopping.join_next().await.is_some() {}
    }
}
