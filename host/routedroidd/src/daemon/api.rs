//! Request dispatch: every `Request` becomes exactly one `Response`.

use std::sync::Arc;

use routedroid_ipc::fault::{Fault, Kind};
use routedroid_ipc::{Request, Response, StartRequest, API_VERSION};

pub use super::run::run_session;
use super::{Daemon, SessionHandle};

pub async fn handle(daemon: &Arc<Daemon>, request: Request) -> Response {
    match request {
        Request::Version => Response::Version { daemon: env!("CARGO_PKG_VERSION").into(), api: API_VERSION },
        Request::Devices => match super::devices::list(daemon).await {
            Ok(devices) => Response::Devices { devices },
            Err(e) => error(e),
        },
        Request::Status => {
            let sessions = daemon.sessions.lock().await.values().map(SessionHandle::info).collect();
            Response::Status { sessions }
        }
        Request::Start(req) => start(daemon, req).await.unwrap_or_else(error),
        Request::Stop { serial } => stop(daemon, &serial).await.unwrap_or_else(error),
        // The connection layer turns on event forwarding; nothing to do here.
        Request::Subscribe => Response::Ok,
    }
}

fn error(e: Fault) -> Response {
    Response::Error { kind: e.kind(), message: e.to_string() }
}

async fn start(daemon: &Arc<Daemon>, req: StartRequest) -> Result<Response, Fault> {
    // Policy first, so a bad request never touches the map.
    crate::device::Transport::check(&req.serial, req.allow_network_adb)?;
    let mut sessions = daemon.sessions.lock().await;
    if sessions.contains_key(&req.serial) {
        return Err(Fault::msg(Kind::Usage, format!("a session is already running on {}", req.serial)));
    }
    if let Some(other) = sessions.values().find(|s| s.phone_ip == req.phone_ip) {
        return Err(Fault::msg(
            Kind::Usage,
            format!("{} is already used by the session on {}", req.phone_ip, other.serial),
        ));
    }
    let tun = match &req.tun {
        Some(t) if sessions.values().any(|s| &s.tun == t) => {
            return Err(Fault::msg(Kind::Usage, format!("TUN {t} is already used by another session")))
        }
        Some(t) => t.clone(),
        None => (0..).map(|n| format!("phone{n}")).find(|t| !sessions.values().any(|s| &s.tun == t)).unwrap(),
    };
    let serial = req.serial.clone();
    sessions.insert(serial.clone(), SessionHandle::spawn(daemon.clone(), req, tun));
    Ok(Response::Started { serial })
}

/// The handle stays in the map (state `Stopping`) until its task has torn
/// everything down, so a concurrent `start` on the serial is refused.
async fn stop(daemon: &Arc<Daemon>, serial: &str) -> Result<Response, Fault> {
    let (stop, state) = {
        let sessions = daemon.sessions.lock().await;
        let handle = sessions.get(serial).ok_or_else(|| Fault::msg(Kind::Usage, format!("no session on {serial}")))?;
        (handle.stop_switch(), handle.state_watch())
    };
    match SessionHandle::stop_and_wait_on(&stop, state).await {
        Some(outcome) if outcome.ok => Ok(Response::Ok),
        Some(outcome) => Err(Fault::msg(outcome.kind.unwrap_or(Kind::Internal), outcome.message)),
        None => {
            Err(Fault::msg(Kind::Internal, format!("session on {serial} is still stopping; watch for its ended event")))
        }
    }
}
