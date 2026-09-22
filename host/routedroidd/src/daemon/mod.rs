//! The daemon: adb, the helper socket, the live device connections and the
//! event bus. It is the only thing that owns connection state, because a
//! device connection outlives the client that asked for it and is shared by
//! every client that watches it. Clients reach it through [`Api`].

mod api;
mod connection;
mod connections;
mod context;
mod events;
mod inventory;

use std::path::PathBuf;
use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind, Result};
use routedroid_ipc::{ConnectionInfo, StartRequest};

use self::connections::DeviceConnections;
use self::context::ConnectionContext;
use self::events::EventBus;
use crate::adb::Adb;
pub use api::Api;
pub use connection::DeviceConnection;

pub struct Daemon {
    adb: Adb,
    helper_socket: Arc<PathBuf>,
    connections: DeviceConnections,
    events: EventBus,
}

impl Daemon {
    pub fn new(adb: Adb, helper_socket: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            adb,
            helper_socket: Arc::new(helper_socket),
            connections: DeviceConnections::default(),
            events: EventBus::new(),
        })
    }

    /// The handle client connections are served through.
    pub fn api(self: &Arc<Self>) -> Api {
        Api::new(self.clone())
    }

    fn context(&self) -> ConnectionContext {
        ConnectionContext {
            adb: self.adb.clone(),
            helper_socket: self.helper_socket.clone(),
            events: self.events.clone(),
            connections: self.connections.clone(),
        }
    }

    pub async fn status(&self) -> Vec<ConnectionInfo> {
        self.connections.info().await
    }

    /// Connect `request.serial`. Returns once the connection's task is
    /// running; its progress arrives as events.
    pub async fn start(&self, request: StartRequest) -> Result<()> {
        // Policy first, so a bad request never touches the connection table.
        crate::device::Transport::check(&request.serial, request.allow_network_adb)?;
        let context = self.context();
        let spawn_request = request.clone();
        self.connections.start(&request, move |tun| DeviceConnection::spawn(context, spawn_request, tun)).await
    }

    /// Ask a device connection to stop and wait for its outcome. The handle
    /// stays in the table (state `Stopping`) until its task has torn
    /// everything down, so a concurrent `start` on the serial is refused.
    pub async fn stop(&self, serial: &str) -> Result<()> {
        let (stop, state) = self
            .connections
            .stop_target(serial)
            .await
            .ok_or_else(|| Fault::msg(Kind::Usage, format!("{serial} is not connected")))?;
        match DeviceConnection::stop_and_wait_on(&stop, state).await {
            Some(outcome) if outcome.ok => Ok(()),
            Some(outcome) => Err(Fault::msg(outcome.kind.unwrap_or(Kind::Internal), outcome.message)),
            None => {
                Err(Fault::msg(Kind::Internal, format!("{serial} is still disconnecting; watch for its ended event")))
            }
        }
    }

    /// Stop every connection and wait for each to end (daemon shutdown).
    pub async fn stop_all(&self) {
        self.events.publish(routedroid_ipc::Event::Shutdown);
        let mut stopping = tokio::task::JoinSet::new();
        for connection in self.connections.take_all().await {
            stopping.spawn(async move { connection.stop_and_wait().await });
        }
        while stopping.join_next().await.is_some() {}
    }
}
