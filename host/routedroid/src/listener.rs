//! Loopback listener: bind on a kernel-chosen port, accept exactly one
//! connection from loopback within a deadline.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tracing::warn;

use crate::fault::{Fault, FaultExt, Kind, Result};

/// Bind `127.0.0.1:0` and report the port the kernel chose.
pub async fn bind_loopback() -> Result<(TcpListener, u16)> {
    let l = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await.fault(Kind::Internal)?;
    let port = l.local_addr().fault(Kind::Internal)?.port();
    Ok((l, port))
}

pub async fn accept_one(listener: &TcpListener, timeout: Duration) -> Result<TcpStream> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(Fault::msg(Kind::Vpn, "the app did not connect in time (did the VPN consent dialog appear?)"));
        }
        let (stream, peer) = tokio::time::timeout(remaining, listener.accept())
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
