//! The TCP endpoint the app connects to. The app dials `127.0.0.1:<device
//! port>` on the phone; adb carries that over USB and connects to this
//! listener on the PC's loopback. Exactly one connection is accepted per
//! session, and only from loopback.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tracing::warn;

use routedroid_ipc::fault::{Fault, FaultExt, Kind, Result};

pub struct AppListener {
    listener: TcpListener,
    port: u16,
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

    /// Wait for the app; consumes the listener so nothing else can connect
    /// afterwards.
    pub async fn accept(self, timeout: Duration) -> Result<TcpStream> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(Fault::msg(
                    Kind::Vpn,
                    "the app did not connect in time (did the VPN consent dialog appear?)",
                ));
            }
            let (stream, peer) = tokio::time::timeout(remaining, self.listener.accept())
                .await
                .map_err(|_| Fault::msg(Kind::Vpn, "the app did not connect in time"))?
                .fault(Kind::Internal)?;
            if !peer.ip().is_loopback() {
                warn!(%peer, "rejected non-loopback connection");
                continue;
            }
            stream.set_nodelay(true).ok();
            return Ok(stream);
        }
    }
}
