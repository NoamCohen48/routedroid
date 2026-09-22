//! What a session's task needs from the daemon — and nothing more: how to
//! reach the phone, where the helper listens, where to publish state, and
//! the session table it removes itself from when it ends. A session never
//! holds the daemon itself, so it cannot call back into the API.

use std::path::PathBuf;
use std::sync::Arc;

use super::events::EventBus;
use super::sessions::Sessions;
use crate::adb::Adb;

#[derive(Clone)]
pub struct SessionContext {
    pub(super) adb: Adb,
    pub(super) helper_socket: Arc<PathBuf>,
    pub(super) events: EventBus,
    pub(super) sessions: Sessions,
}
