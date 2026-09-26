//! The TCP endpoint the app connects to. The app dials `127.0.0.1:<device
//! port>` on the phone; adb carries that over USB to this listener on the
//! PC's loopback. Every app on the phone can reach that port (and every
//! local user this one), so a connection is taken for the app only once it
//! opens with a HELLO naming this session and port. Anything else is
//! screened on its own task, answered with ERROR and closed, while the
//! listener keeps accepting: a squatter cannot take the only slot.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use routedroid_proto::frame::{self, Frame, MessageType};
use routedroid_proto::messages::{self, ErrorBody, ErrorCode, Hello};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::{sleep_until, timeout, Instant};
use tracing::{debug, info, warn};

use routedroid_ipc::fault::{Fault, FaultExt, Kind, Result};

/// How long a connection has to send its HELLO.
const SCREEN_DEADLINE: Duration = Duration::from_secs(5);
/// Connections screened at once, and in total, before giving up.
const MAX_SCREENING: usize = 4;
const MAX_ATTEMPTS: u32 = 64;

pub struct AppListener {
    listener: TcpListener,
    port: u16,
}

/// What the app's HELLO must name (the machine checks the rest).
#[derive(Debug, Clone)]
pub struct Expected {
    pub session: String,
    pub device_port: u16,
    pub mtu: u32,
}

/// The app's connection and the HELLO it opened with.
pub struct Candidate {
    pub stream: TcpStream,
    pub hello: Frame,
}

impl AppListener {
    /// Bind on loopback with a kernel-chosen port.
    pub async fn bind() -> Result<Self> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await.fault(Kind::Internal)?;
        let port = listener.local_addr().fault(Kind::Internal)?.port();
        Ok(Self { listener, port })
    }

    /// The host port to hand to `adb reverse`.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Wait for the app; consumes the listener, so nothing can connect after.
    pub async fn accept(self, wait: Duration, expected: &Expected) -> Result<Candidate> {
        let deadline = Instant::now() + wait;
        let mut screening = JoinSet::new();
        let mut attempts = 0;
        loop {
            if attempts == MAX_ATTEMPTS && screening.is_empty() {
                return Err(Fault::msg(Kind::Protocol, format!("{MAX_ATTEMPTS} connections, none of them the app")));
            }
            tokio::select! {
                accepted = self.listener.accept(), if attempts < MAX_ATTEMPTS && screening.len() < MAX_SCREENING => {
                    match accepted {
                        Ok((stream, peer)) => {
                            attempts += 1;
                            debug!(%peer, "screening a connection");
                            screening.spawn(screen(stream, expected.clone()));
                        }
                        Err(error) => warn!(%error, "accept failed"),
                    }
                }
                Some(screened) = screening.join_next() => {
                    if let Ok(Some(candidate)) = screened {
                        if attempts > 1 {
                            info!(others = attempts - 1, "the app got through past other connections");
                        }
                        return Ok(candidate);
                    }
                }
                () = sleep_until(deadline) => {
                    return Err(Fault::msg(Kind::Vpn, "the app did not connect in time"));
                }
            }
        }
    }
}

/// The app, or `None` after telling the peer why not.
async fn screen(mut stream: TcpStream, expected: Expected) -> Option<Candidate> {
    stream.set_nodelay(true).ok();
    let refusal = match timeout(SCREEN_DEADLINE, frame::read_frame(&mut stream, expected.mtu)).await {
        Err(_) => ErrorBody::new(ErrorCode::ProtocolError, "no HELLO in time"),
        Ok(Err(e)) => ErrorBody::new(ErrorCode::ProtocolError, e.to_string()),
        Ok(Ok(first)) if first.message_type != MessageType::Hello => {
            ErrorBody::new(ErrorCode::ProtocolError, format!("{} before HELLO", first.message_type))
        }
        Ok(Ok(first)) => match messages::parse::<Hello>(&first.body) {
            Ok(hello) if hello.session == expected.session && hello.device_port == expected.device_port => {
                return Some(Candidate { stream, hello: first });
            }
            Ok(_) => ErrorBody::new(ErrorCode::SessionMismatch, "HELLO session/port is not the one launched"),
            Err(e) => ErrorBody::new(ErrorCode::ProtocolError, format!("HELLO: {e}")),
        },
    };
    warn!(peer = ?stream.peer_addr().ok(), reason = %refusal.message, "a connection that is not the app");
    let _ = timeout(SCREEN_DEADLINE, stream.write_all(&Frame::json(MessageType::Error, &refusal).encode())).await;
    None
}

#[cfg(test)]
mod tests;
