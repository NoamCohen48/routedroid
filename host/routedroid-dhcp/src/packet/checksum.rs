//! RFC 1071 Internet checksums.

use std::net::Ipv4Addr;

use super::IPPROTO_UDP;

/// Ones-complement sum, folded and inverted; an odd trailing byte is padded
/// with zero. In host order; callers store it big-endian.
pub fn checksum(data: &[u8]) -> u16 {
    !fold(sum(data))
}

fn sum(data: &[u8]) -> u32 {
    let (pairs, rest) = data.as_chunks::<2>();
    let mut s = pairs.iter().fold(0u32, |s, c| {
        s.wrapping_add(u32::from(u16::from_be_bytes(*c)))
    });
    if let [last] = rest {
        s = s.wrapping_add(u32::from(u16::from_be_bytes([*last, 0])));
    }
    s
}

fn fold(mut s: u32) -> u16 {
    while s > 0xffff {
        s = (s & 0xffff) + (s >> 16);
    }
    u16::try_from(s).unwrap_or(u16::MAX)
}

/// UDP checksum over the IPv4 pseudo-header, the UDP header with its
/// checksum field taken as zero, and the payload. Never 0, which on the
/// wire means "no checksum". `udp` is at most one IPv4 packet long.
pub fn udp_checksum(src: Ipv4Addr, dst: Ipv4Addr, udp: &[u8]) -> u16 {
    let mut pseudo = [0u8; 12];
    pseudo[..4].copy_from_slice(&src.octets());
    pseudo[4..8].copy_from_slice(&dst.octets());
    pseudo[9] = IPPROTO_UDP;
    let len = u16::try_from(udp.len()).unwrap_or(u16::MAX);
    pseudo[10..12].copy_from_slice(&len.to_be_bytes());
    let s = sum(&pseudo)
        .wrapping_add(sum(&udp[..6]))
        .wrapping_add(sum(&udp[8..]));
    match !fold(s) {
        0 => 0xffff,
        c => c,
    }
}
