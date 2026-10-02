//! Hand-built Ethernet / IPv4 / UDP / ARP frames and their strict parsers.
//! Nothing here touches a socket.

mod arp;
mod checksum;
mod error;
mod udp;

pub use arp::{ARP_REPLY, ARP_REQUEST, Arp, announce_frame, parse_arp, probe_frame, reply_frame};
pub use checksum::{checksum, udp_checksum};
pub use error::PacketError;
pub use udp::{UdpFrame, ipv4_udp_frame, parse_udp};

pub const ETH_HDR: usize = 14;
pub const IPV4_HDR: usize = 20;
pub const UDP_HDR: usize = 8;
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const IPPROTO_UDP: u8 = 17;
pub const BROADCAST_MAC: Mac = [0xff; 6];

pub type Mac = [u8; 6];

pub fn fmt_mac(m: &Mac) -> String {
    let [a, b, c, d, e, f] = m;
    format!("{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}")
}

pub fn parse_mac(s: &str) -> Option<Mac> {
    let mut out = [0u8; 6];
    let mut parts = s.split(':');
    for byte in &mut out {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

pub fn ethertype(frame: &[u8]) -> Option<u16> {
    (frame.len() >= ETH_HDR).then(|| u16::from_be_bytes([frame[12], frame[13]]))
}

/// The six bytes at `at`; the caller has checked the length.
fn mac_at(frame: &[u8], at: usize) -> Mac {
    let mut m = [0u8; 6];
    m.copy_from_slice(&frame[at..at + 6]);
    m
}

#[cfg(test)]
mod tests;
