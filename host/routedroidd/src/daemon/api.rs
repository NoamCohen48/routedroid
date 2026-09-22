//! Request dispatch: every `Request` becomes exactly one `Response`.

use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind};
use routedroid_ipc::{Request, Response, SessionInfo, StartRequest, API_VERSION};

use super::{Daemon, SessionHandle};

impl Daemon {
    /// Answer one request. Takes `&Arc<Self>` because starting a session
    /// hands the session's task its own handle on the daemon.
    pub async fn handle(self: &Arc<Self>, request: Request) -> Response {
        match request {
            Request::Version => Response::Version { daemon: env!("CARGO_PKG_VERSION").into(), api: API_VERSION },
            Request::Devices => match self.devices().await {
                Ok(devices) => Response::Devices { devices },
                Err(fault) => error(fault),
            },
            Request::Status => Response::Status { sessions: self.status().await },
            Request::Start(request) => self.start(request).await.unwrap_or_else(error),
            Request::Stop { serial } => self.stop(&serial).await.unwrap_or_else(error),
            // The connection layer turns on event forwarding; nothing to do here.
            Request::Subscribe => Response::Ok,
        }
    }

    pub async fn status(&self) -> Vec<SessionInfo> {
        self.sessions().await.values().map(SessionHandle::info).collect()
    }

    async fn start(self: &Arc<Self>, request: StartRequest) -> Result<Response, Fault> {
        // Policy first, so a bad request never touches the map.
        crate::device::Transport::check(&request.serial, request.allow_network_adb)?;
        let mut sessions = self.sessions().await;
        if sessions.contains_key(&request.serial) {
            return Err(Fault::msg(Kind::Usage, format!("a session is already running on {}", request.serial)));
        }
        if let Some(other) = sessions.values().find(|session| session.phone_ip == request.phone_ip) {
            return Err(Fault::msg(
                Kind::Usage,
                format!("{} is already used by the session on {}", request.phone_ip, other.serial),
            ));
        }
        let taken = |name: &String| sessions.values().any(|session| &session.tun == name);
        let tun = match &request.tun {
            Some(name) if taken(name) => {
                return Err(Fault::msg(Kind::Usage, format!("TUN {name} is already used by another session")))
            }
            Some(name) => name.clone(),
            None => (0..).map(|n| format!("phone{n}")).find(|name| !taken(name)).unwrap(),
        };
        let serial = request.serial.clone();
        sessions.insert(serial.clone(), SessionHandle::spawn(self.clone(), request, tun));
        Ok(Response::Started { serial })
    }

    /// The handle stays in the map (state `Stopping`) until its task has torn
    /// everything down, so a concurrent `start` on the serial is refused.
    async fn stop(&self, serial: &str) -> Result<Response, Fault> {
        let (stop, state) = {
            let sessions = self.sessions().await;
            let handle =
                sessions.get(serial).ok_or_else(|| Fault::msg(Kind::Usage, format!("no session on {serial}")))?;
            (handle.stop_switch(), handle.state_watch())
        };
        match SessionHandle::stop_and_wait_on(&stop, state).await {
            Some(outcome) if outcome.ok => Ok(Response::Ok),
            Some(outcome) => Err(Fault::msg(outcome.kind.unwrap_or(Kind::Internal), outcome.message)),
            None => Err(Fault::msg(
                Kind::Internal,
                format!("session on {serial} is still stopping; watch for its ended event"),
            )),
        }
    }
}

fn error(fault: Fault) -> Response {
    Response::Error { kind: fault.kind(), message: fault.to_string() }
}
