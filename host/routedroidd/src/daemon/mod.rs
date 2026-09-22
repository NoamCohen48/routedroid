//! The daemon: adb, the helper socket, the live sessions and the event bus.
//! It is the only thing that owns session state. Clients reach it through
//! [`Api`]; sessions get a [`SessionContext`], never the daemon itself.

mod api;
mod context;
mod devices;
mod events;
mod run;
mod session;
mod sessions;

use std::path::PathBuf;
use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind, Result};
use routedroid_ipc::{SessionInfo, StartRequest};

use self::context::SessionContext;
use self::events::EventBus;
use self::sessions::Sessions;
use crate::adb::Adb;
pub use api::Api;
pub use session::SessionHandle;

pub struct Daemon {
    adb: Adb,
    helper_socket: Arc<PathBuf>,
    sessions: Sessions,
    events: EventBus,
}

impl Daemon {
    pub fn new(adb: Adb, helper_socket: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            adb,
            helper_socket: Arc::new(helper_socket),
            sessions: Sessions::default(),
            events: EventBus::new(),
        })
    }

    /// The handle clients are served through.
    pub fn api(self: &Arc<Self>) -> Api {
        Api::new(self.clone())
    }

    fn context(&self) -> SessionContext {
        SessionContext {
            adb: self.adb.clone(),
            helper_socket: self.helper_socket.clone(),
            events: self.events.clone(),
            sessions: self.sessions.clone(),
        }
    }

    pub async fn status(&self) -> Vec<SessionInfo> {
        self.sessions.info().await
    }

    /// Start a session on `request.serial`. Returns once the session task is
    /// running; its progress arrives as events.
    pub async fn start(&self, request: StartRequest) -> Result<()> {
        // Policy first, so a bad request never touches the session table.
        crate::device::Transport::check(&request.serial, request.allow_network_adb)?;
        let context = self.context();
        let spawn_request = request.clone();
        self.sessions.start(&request, move |tun| SessionHandle::spawn(context, spawn_request, tun)).await
    }

    /// Ask a session to stop and wait for its outcome. The handle stays in
    /// the table (state `Stopping`) until its task has torn everything down,
    /// so a concurrent `start` on the serial is refused.
    pub async fn stop(&self, serial: &str) -> Result<()> {
        let (stop, state) = self
            .sessions
            .stop_target(serial)
            .await
            .ok_or_else(|| Fault::msg(Kind::Usage, format!("no session on {serial}")))?;
        match SessionHandle::stop_and_wait_on(&stop, state).await {
            Some(outcome) if outcome.ok => Ok(()),
            Some(outcome) => Err(Fault::msg(outcome.kind.unwrap_or(Kind::Internal), outcome.message)),
            None => Err(Fault::msg(
                Kind::Internal,
                format!("session on {serial} is still stopping; watch for its ended event"),
            )),
        }
    }

    /// Stop every session and wait for each to end (daemon shutdown).
    pub async fn stop_all(&self) {
        self.events.publish(routedroid_ipc::Event::Shutdown);
        let mut stopping = tokio::task::JoinSet::new();
        for handle in self.sessions.take_all().await {
            stopping.spawn(async move { handle.stop_and_wait().await });
        }
        while stopping.join_next().await.is_some() {}
    }
}
