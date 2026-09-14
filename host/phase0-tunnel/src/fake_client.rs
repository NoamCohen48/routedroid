//! In-process fake Android client used by `selftest` and tests. Speaks the
//! Android side of protocol/phase0-draft.md over a plain TCP socket.

use std::net::SocketAddr;

use anyhow::{bail, Context, Result};
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;

use crate::frame::{read_frame, Frame, MessageType};
use crate::ipv4;
use crate::messages::{ConfigureVpn, Hello, HelloAck, VpnReady};

#[derive(Debug)]
pub struct FakeReport {
    pub hello_ack: HelloAck,
    pub configure: ConfigureVpn,
    /// The IP_PACKET body the host sent back for our probe packet.
    pub echoed: Vec<u8>,
    pub got_pong: bool,
}

/// Build a minimal IPv4/ICMP echo request `src -> dst` with `payload_len` bytes.
pub fn icmp_echo(src: [u8; 4], dst: [u8; 4], payload_len: usize) -> Vec<u8> {
    let total = 20 + 8 + payload_len;
    let mut p = vec![0u8; total];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    p[8] = 64; // TTL
    p[9] = 1; // ICMP
    p[12..16].copy_from_slice(&src);
    p[16..20].copy_from_slice(&dst);
    let csum = checksum(&p[..20]);
    p[10..12].copy_from_slice(&csum.to_be_bytes());
    p[20] = 8; // echo request
    for (i, b) in p[28..].iter_mut().enumerate() {
        *b = i as u8;
    }
    let icsum = checksum(&p[20..]);
    p[22..24].copy_from_slice(&icsum.to_be_bytes());
    p
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 { u16::from_be_bytes([chunk[0], chunk[1]]) } else { u16::from(chunk[0]) << 8 };
        sum += u32::from(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Swap IPv4 source and destination (what a fake "LAN" does to bounce a packet).
pub fn swap_addresses(pkt: &mut [u8]) {
    let (a, b) = pkt.split_at_mut(16);
    a[12..16].swap_with_slice(&mut b[..4]);
}

/// Connect, complete the handshake, exchange one packet and a PING, then STOP.
pub async fn run_fake_android(
    addr: SocketAddr,
    session: &str,
    device_port: u16,
    probe: Vec<u8>,
) -> Result<FakeReport> {
    let mut stream = TcpStream::connect(addr).await.context("connect to host port")?;
    let mtu_guess = crate::frame::DEFAULT_MTU;

    let hello = Frame::json(MessageType::Hello, &Hello { protocol: 0, session: session.into(), device_port });
    stream.write_all(&hello.encode()).await?;

    let f = read_frame(&mut stream, mtu_guess).await.context("read HELLO_ACK")?;
    if f.message_type != MessageType::HelloAck {
        bail!("expected HELLO_ACK, got {:?}", f.message_type);
    }
    let hello_ack: HelloAck = serde_json::from_slice(&f.body).context("parse HELLO_ACK")?;
    if hello_ack.protocol != 0 {
        bail!("HELLO_ACK protocol {}", hello_ack.protocol);
    }
    let mtu = hello_ack.mtu;

    let f = read_frame(&mut stream, mtu).await.context("read CONFIGURE_VPN")?;
    if f.message_type != MessageType::ConfigureVpn {
        bail!("expected CONFIGURE_VPN, got {:?}", f.message_type);
    }
    let configure: ConfigureVpn = serde_json::from_slice(&f.body).context("parse CONFIGURE_VPN")?;
    if configure.mtu != mtu {
        bail!("CONFIGURE_VPN mtu {} != HELLO_ACK mtu {mtu}", configure.mtu);
    }

    let ready = VpnReady {
        addresses: configure.addresses.iter().map(|p| format!("{}/{}", p.address, p.prefix)).collect(),
        mtu,
    };
    stream.write_all(&Frame::json(MessageType::VpnReady, &ready).encode()).await?;

    // Android VPN read -> IP_PACKET -> host.
    ipv4::validate(&probe).context("probe packet")?;
    stream.write_all(&Frame::ip_packet(probe).encode()).await?;
    stream.write_all(&Frame::empty(MessageType::Ping).encode()).await?;

    let mut echoed = None;
    let mut got_pong = false;
    while echoed.is_none() || !got_pong {
        let f = read_frame(&mut stream, mtu).await.context("read reply")?;
        match f.message_type {
            MessageType::IpPacket => {
                ipv4::validate(&f.body).context("host sent invalid IPv4")?;
                echoed = Some(f.body);
            }
            MessageType::Pong => got_pong = true,
            MessageType::Ping => stream.write_all(&Frame::empty(MessageType::Pong).encode()).await?,
            MessageType::Error => bail!("host sent ERROR: {}", String::from_utf8_lossy(&f.body)),
            other => bail!("unexpected {other:?} while Active"),
        }
    }

    stream.write_all(&Frame::empty(MessageType::Stop).encode()).await?;
    stream.shutdown().await.ok();
    Ok(FakeReport { hello_ack, configure, echoed: echoed.unwrap_or_default(), got_pong })
}

/// A misbehaving client: sends IP_PACKET before HELLO and returns the host's
/// reaction (the ERROR frame body, if any, and whether the host closed).
pub async fn run_misbehaving_client(addr: SocketAddr) -> Result<(Option<String>, bool)> {
    let mut stream = TcpStream::connect(addr).await?;
    let pkt = icmp_echo([10, 0, 0, 2], [10, 0, 0, 1], 8);
    stream.write_all(&Frame::ip_packet(pkt).encode()).await?;
    let mut error = None;
    let closed = loop {
        match read_frame(&mut stream, crate::frame::DEFAULT_MTU).await {
            Ok(f) if f.message_type == MessageType::Error => {
                error = Some(String::from_utf8_lossy(&f.body).into_owned());
            }
            Ok(other) => bail!("unexpected frame {:?}", other.message_type),
            Err(crate::frame::FrameError::Truncated { .. }) | Err(crate::frame::FrameError::Io(_)) => break true,
            Err(e) => bail!("decode error: {e}"),
        }
    };
    Ok((error, closed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icmp_echo_is_valid_ipv4() {
        let p = icmp_echo([10, 0, 0, 2], [10, 0, 0, 1], 32);
        assert_eq!(p.len(), 60);
        assert_eq!(ipv4::validate(&p), Ok(()));
        // Header checksum verifies to zero.
        assert_eq!(checksum(&p[..20]), 0);
        let mut q = p.clone();
        swap_addresses(&mut q);
        assert_eq!(ipv4::source(&q), std::net::Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(ipv4::destination(&q), std::net::Ipv4Addr::new(10, 0, 0, 2));
    }
}
