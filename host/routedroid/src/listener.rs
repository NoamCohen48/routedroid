//! Accept exactly one connection from loopback within a deadline.

use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tracing::warn;

use crate::fault::{Fault, FaultExt, Kind, Result};

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
