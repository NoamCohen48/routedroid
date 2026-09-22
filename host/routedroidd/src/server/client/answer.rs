//! One request, one response: the whole protocol translation. This is the
//! only place in the daemon that knows the wire's types exist.

use routedroid_ipc::fault::Fault;
use routedroid_ipc::{DeviceInfo, Request, Response, API_VERSION};

use super::super::view;
use super::ClientConnection;

impl ClientConnection {
    pub(super) async fn answer(&self, request: Request) -> Response {
        match request {
            Request::Version => Response::Version { daemon: env!("CARGO_PKG_VERSION").into(), api: API_VERSION },
            Request::Devices => Response::Devices { devices: self.devices_view().await },
            Request::Status => Response::Status { connections: self.connections.info().await },
            Request::Start(start) => {
                let serial = start.serial.clone();
                match self.connections.start(start).await {
                    Ok(()) => Response::Started { serial },
                    Err(fault) => error(fault),
                }
            }
            Request::Stop { serial } => match self.connections.stop(&serial).await {
                Ok(()) => Response::Ok,
                Err(fault) => error(fault),
            },
            // We subscribed while reading the line; nothing else to do.
            Request::Subscribe => Response::Ok,
        }
    }

    /// What adb reports, joined with the connections we have on those phones.
    pub(super) async fn devices_view(&self) -> Vec<DeviceInfo> {
        view::devices(&self.devices.current(), &self.connections.states().await)
    }
}

fn error(fault: Fault) -> Response {
    Response::Error { kind: fault.kind(), message: fault.to_string() }
}
