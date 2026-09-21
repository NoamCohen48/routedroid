//! Port choice: the host port is whatever the loopback listener gets; the
//! device port is picked from a fixed range, skipping anything `adb reverse
//! --list` already shows for this device.

use std::net::{Ipv4Addr, SocketAddrV4};

use tokio::net::TcpListener;

use crate::fault::{FaultExt, Kind, Result};

pub const DEVICE_PORT_RANGE: std::ops::RangeInclusive<u16> = 17_000..=17_999;

/// Bind `127.0.0.1:0` and report the port the kernel chose.
pub async fn bind_loopback() -> Result<(TcpListener, u16)> {
    let l = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await.fault(Kind::Internal)?;
    let port = l.local_addr().fault(Kind::Internal)?.port();
    Ok((l, port))
}

/// First port in the range not in `used`, starting from a random offset so
/// two controllers racing for the same device rarely collide.
pub fn pick_device_port(used: &[u16], seed: u16) -> Option<u16> {
    let len = DEVICE_PORT_RANGE.end() - DEVICE_PORT_RANGE.start() + 1;
    (0..len).map(|i| DEVICE_PORT_RANGE.start() + (seed.wrapping_add(i)) % len).find(|p| !used.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_used_ports_and_wraps() {
        assert_eq!(pick_device_port(&[], 0), Some(17_000));
        assert_eq!(pick_device_port(&[17_000], 0), Some(17_001));
        assert_eq!(pick_device_port(&[17_999], 999), Some(17_000));
        let all: Vec<u16> = DEVICE_PORT_RANGE.collect();
        assert_eq!(pick_device_port(&all, 5), None);
    }
}
