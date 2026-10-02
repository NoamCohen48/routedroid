//! One request, one response: the whole protocol translation. This is the
//! only place in the daemon that knows the wire's types exist.

use std::future::Future;

use routedroid_ipc::wire::ClientMessage;
use routedroid_ipc::{API_VERSION, DeviceInfo, Request, Response};
use tokio::sync::watch;

use super::super::view;
use crate::daemon::{AttachedDevices, DeviceConnections, Snapshot};
use crate::fault::{Fault, Kind};

/// What a client connection may ask of the daemon.
pub trait Answer: Clone + Send + Sync + 'static {
    /// Any request but `Subscribe`, which the connection handles itself.
    fn answer(&self, request: Request) -> impl Future<Output = Response> + Send;
    fn devices_view(&self) -> impl Future<Output = Vec<DeviceInfo>> + Send;
    fn device_changes(&self) -> watch::Receiver<Snapshot>;
}

/// The two components a client may reach, and nothing else.
#[derive(Clone)]
pub struct Handles {
    pub devices: AttachedDevices,
    pub connections: DeviceConnections,
}

impl Answer for Handles {
    async fn answer(&self, request: Request) -> Response {
        match request {
            Request::Version => Response::Version {
                daemon: env!("CARGO_PKG_VERSION").into(),
                api: API_VERSION,
            },
            Request::Devices => Response::Devices {
                devices: self.devices_view().await,
            },
            Request::Status => Response::Status {
                connections: self.connections.info(),
            },
            Request::Interfaces => match self.connections.interfaces().await {
                Ok(interfaces) => Response::Interfaces { interfaces },
                Err(fault) => error(fault),
            },
            Request::Start(start) => {
                let serial = start.serial.clone();
                match self.connections.start(start).await {
                    Ok(tun) => Response::Started {
                        serial,
                        tun: tun.to_string(),
                    },
                    Err(fault) => error(fault),
                }
            }
            Request::Stop { serial } => match self.connections.stop(&serial).await {
                Ok(outcome) => Response::Stopped { serial, outcome },
                Err(fault) => error(fault),
            },
            Request::Subscribe => Response::Subscribed,
        }
    }

    /// What adb reports, joined with the connections we have on those phones.
    async fn devices_view(&self) -> Vec<DeviceInfo> {
        view::devices(&self.devices.current(), &self.connections.states())
    }

    fn device_changes(&self) -> watch::Receiver<Snapshot> {
        self.devices.changes()
    }
}

/// A request line, or the id to answer (0 when there is none) and why not.
/// A request this daemon does not know (from a newer client) is an answer,
/// not a reason to hang up.
pub fn parse(line: &str) -> Result<ClientMessage, (u64, Response)> {
    serde_json::from_str(line).map_err(|e| {
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("id")?.as_u64())
            .unwrap_or(0);
        (
            id,
            Response::Error {
                kind: Kind::Usage,
                message: format!("bad request: {e}"),
            },
        )
    })
}

fn error(fault: Fault) -> Response {
    Response::Error {
        kind: fault.kind(),
        message: fault.to_string(),
    }
}
