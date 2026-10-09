//! One request, one response: the whole protocol translation. This is the
//! only place in the daemon that knows the wire's types exist.

use std::future::Future;

use routedroid_ipc::wire::ClientMessage;
use routedroid_ipc::{API_VERSION, DeviceInfo, Event, Request, Response};
use tokio::sync::watch;

use super::super::view;
use crate::daemon::{AttachedDevices, DeviceConnections, Snapshot, doctor};
use crate::fault::{Fault, Kind};
use crate::notify::Setting;

/// What a client connection may ask of the daemon.
pub trait Answer: Clone + Send + Sync + 'static {
    /// Any request but `Subscribe`, which the connection handles itself.
    fn answer(&self, request: Request) -> impl Future<Output = Response> + Send;
    fn devices_view(&self) -> impl Future<Output = Vec<DeviceInfo>> + Send;
    fn device_changes(&self) -> watch::Receiver<Snapshot>;
}

/// The two components a client may reach, and the notifications setting.
#[derive(Clone)]
pub struct Handles {
    pub devices: AttachedDevices,
    pub connections: DeviceConnections,
    pub notifications: Setting,
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
            Request::Start(start) => match self.connections.start(start).await {
                Ok(accepted) => Response::Started {
                    name: self
                        .connections
                        .phones()
                        .find(&accepted.serial)
                        .and_then(|p| p.name),
                    serial: accepted.serial,
                    lan_if: accepted.lan_if,
                    phone_ip: accepted.phone_ip,
                    tun: accepted.tun.to_string(),
                },
                Err(fault) => error(fault),
            },
            Request::Phones => Response::Phones {
                phones: self.connections.phones().all(),
            },
            Request::Remember(phone) => match self.connections.phones().remember(phone) {
                Ok(phone) => {
                    self.devices_changed().await;
                    Response::Remembered { phone }
                }
                Err(fault) => error(fault),
            },
            Request::Forget { phone } => match self.connections.phones().forget(&phone) {
                Ok(phone) => {
                    self.devices_changed().await;
                    Response::Forgotten { phone }
                }
                Err(fault) => error(fault),
            },
            Request::Stop { serial } => match self.connections.stop(&serial).await {
                Ok(outcome) => Response::Stopped { serial, outcome },
                Err(fault) => error(fault),
            },
            Request::Doctor { repair } => {
                let socket = self.connections.helper_socket();
                let (checks, done) = doctor::run(&self.devices, socket, repair).await;
                Response::Doctor { checks, done }
            }
            Request::Notifications { on } => {
                match on.map_or(Ok(()), |on| self.notifications.set(on)) {
                    Ok(()) => Response::Notifications {
                        on: self.notifications.on(),
                    },
                    Err(fault) => error(fault),
                }
            }
            Request::Subscribe => Response::Subscribed,
        }
    }

    /// What adb reports, joined with the connections we have on those phones.
    async fn devices_view(&self) -> Vec<DeviceInfo> {
        let phones = self.connections.phones().all();
        view::devices(&self.devices.current(), &self.connections.states(), &phones)
    }

    fn device_changes(&self) -> watch::Receiver<Snapshot> {
        self.devices.changes()
    }
}

impl Handles {
    /// A name or auto-connect changed: every subscriber's device list is stale.
    async fn devices_changed(&self) {
        let devices = self.devices_view().await;
        self.connections
            .events()
            .publish(Event::Devices { devices });
    }
}

/// A request line, or the id to answer (0 when there is none) and why not.
/// A request this daemon does not know (from a newer client) is an answer,
/// not a reason to hang up.
pub fn parse(line: &str) -> Result<ClientMessage, Box<(u64, Response)>> {
    serde_json::from_str(line).map_err(|e| {
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("id")?.as_u64())
            .unwrap_or(0);
        Box::new((
            id,
            Response::Error {
                kind: Kind::Usage,
                message: format!("bad request: {e}"),
            },
        ))
    })
}

fn error(fault: Fault) -> Response {
    Response::Error {
        kind: fault.kind(),
        message: fault.to_string(),
    }
}
