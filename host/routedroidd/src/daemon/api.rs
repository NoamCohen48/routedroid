//! The client-facing handle: turns one `Request` into exactly one
//! `Response`, and hands out event subscriptions. A connection holds this,
//! not the daemon, so the protocol layer can reach nothing else.

use std::sync::Arc;

use routedroid_ipc::fault::Fault;
use routedroid_ipc::{Event, Request, Response, API_VERSION};
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

    pub async fn request(&self, request: Request) -> Response {
        match request {
            Request::Version => Response::Version { daemon: env!("CARGO_PKG_VERSION").into(), api: API_VERSION },
            Request::Devices => match self.daemon.devices().await {
                Ok(devices) => Response::Devices { devices },
                Err(fault) => error(fault),
            },
            Request::Status => Response::Status { sessions: self.daemon.status().await },
            Request::Start(start) => {
                let serial = start.serial.clone();
                match self.daemon.start(start).await {
                    Ok(()) => Response::Started { serial },
                    Err(fault) => error(fault),
                }
            }
            Request::Stop { serial } => match self.daemon.stop(&serial).await {
                Ok(()) => Response::Ok,
                Err(fault) => error(fault),
            },
            // The connection layer turns on event forwarding; nothing to do here.
            Request::Subscribe => Response::Ok,
        }
    }
}

fn error(fault: Fault) -> Response {
    Response::Error { kind: fault.kind(), message: fault.to_string() }
}
