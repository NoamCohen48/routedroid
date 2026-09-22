//! What a client may ask the daemon to do. A client connection holds this
//! and nothing else: it cannot reach adb, the connection table or the
//! shutdown path. The wire's `Request`/`Response` never appear here — that
//! translation belongs to the connection that speaks the protocol.

use std::sync::Arc;

use routedroid_ipc::fault::Result;
use routedroid_ipc::{ConnectionInfo, DeviceInfo, Event, StartRequest};
use tokio::sync::broadcast;

use super::Daemon;

#[derive(Clone)]
pub struct Api {
    daemon: Arc<Daemon>,
}

impl Api {
    pub(super) fn new(daemon: Arc<Daemon>) -> Self {
        Self { daemon }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.daemon.events.subscribe()
    }

    pub async fn devices(&self) -> Result<Vec<DeviceInfo>> {
        self.daemon.devices().await
    }

    pub async fn status(&self) -> Vec<ConnectionInfo> {
        self.daemon.status().await
    }

    pub async fn start(&self, request: StartRequest) -> Result<()> {
        self.daemon.start(request).await
    }

    pub async fn stop(&self, serial: &str) -> Result<()> {
        self.daemon.stop(serial).await
    }
}
