//! What a device connection's task needs from the daemon — and nothing
//! more: how to reach the phone, where the helper listens, where to publish
//! state, and the table it removes itself from when it ends. A connection
//! never holds the daemon itself, so it cannot call back into the API.

use std::path::PathBuf;
use std::sync::Arc;

use super::connections::DeviceConnections;
use super::events::EventBus;
use crate::adb::Adb;

#[derive(Clone)]
pub struct ConnectionContext {
    pub(super) adb: Adb,
    pub(super) helper_socket: Arc<PathBuf>,
    pub(super) events: EventBus,
    pub(super) connections: DeviceConnections,
}
