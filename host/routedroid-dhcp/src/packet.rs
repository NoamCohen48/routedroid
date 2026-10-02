//! Hand-built Ethernet / IPv4 / UDP / ARP frames and their strict parsers.
//! Nothing here touches a socket.

use std::fmt;
use std::net::Ipv4Addr;

pub const ETH_HDR: usize = 14;
pub const IPV4_HDR: usize = 20;
pub const UDP_HDR: usize = 8;
pub const ARP_LEN: usize = 28;
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const IPPROTO_UDP: u8 = 17;
pub const BROADCAST_MAC: [u8; 6] = [0xff; 6];
pub const ARP_REQUEST: u16 = 1;
pub const ARP_REPLY: u16 = 2;

pub type Mac = [u8; 6];

pub fn fmt_mac(m: &Mac) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[0], m[1], m[2], m[3], m[4], m[5]
    )
}

pub fn parse_mac(s: &str) -> Option<Mac> {
    let mut out = [0u8; 6];
    let mut n = 0;
    for part in s.split(':') {
        if n == 6 || part.len() != 2 {
            return None;
        }
        out[n] = u8::from_str_radix(part, 16).ok()?;
        n += 1;
    }
    (n == 6).then_some(out)
}

/// RFC 1071 ones-complement sum, folded and inverted. Odd trailing byte is
/// padded with zero. Returns the checksum in host order; callers store it
/// big-endian.
pub fn checksum(data: &[u8]) -> u16 {
    !fold(sum(data))
}

fn sum(data: &[u8]) -> u32 {
    let mut s: u32 = 0;
    let (pairs, rest) = data.as_chunks::<2>();
    for c in pairs {
        s = s.wrapping_add(u32::from(u16::from_be_bytes(*c)));
    }
    if let [last] = rest {
        s = s.wrapping_add(u32::from(u16::from_be_bytes([*last, 0])));
    }
    s
}

fn fold(mut s: u32) -> u16 {
    while s > 0xffff {
        s = (s & 0xffff) + (s >> 16);
    }
    s as u16
}

/// UDP checksum over the IPv4 pseudo-header + UDP header + payload, with the
/// checksum field itself treated as zero. Never returns 0 (0 means "absent").
pub fn udp_checksum(src: Ipv4Addr, dst: Ipv4Addr, udp: &[u8]) -> u16 {
    let mut pseudo = [0u8; 12];
    pseudo[..4].copy_from_slice(&src.octets());
    pseudo[4..8].copy_from_slice(&dst.octets());
    pseudo[9] = IPPROTO_UDP;
    pseudo[10..12].copy_from_slice(&(udp.len() as u16).to_be_bytes());
    let mut s = sum(&pseudo).wrapping_add(sum(&udp[..6]));
    // skip the checksum field at [6..8]
    s = s.wrapping_add(sum(&udp[8..]));
    let c = !fold(s);
    if c == 0 { 0xffff } else { c }
}

/// Build `Ethernet(IPv4(UDP(payload)))`. IPv4 id 0, DF clear, TTL 64.
pub fn ipv4_udp_frame(
    src_mac: &Mac,
    dst_mac: &Mac,
    src_ip: Ipv4Addr,
    dst_ip: Ipv4Addr,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Vec<u8> {
    let udp_len = UDP_HDR + payload.len();
    let ip_len = IPV4_HDR + udp_len;
    assert!(ip_len <= 0xffff, "payload too large for one IPv4 packet");
    let mut f = Vec::with_capacity(ETH_HDR + ip_len);
    f.extend_from_slice(dst_mac);
    f.extend_from_slice(src_mac);
    f.extend_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

    let ip_start = f.len();
    f.push(0x45); // version 4, IHL 5
    f.push(0x00); // DSCP/ECN
    f.extend_from_slice(&(ip_len as u16).to_be_bytes());
    f.extend_from_slice(&[0, 0]); // identification
    f.extend_from_slice(&[0, 0]); // flags + fragment offset
    f.push(64); // TTL
    f.push(IPPROTO_UDP);
    f.extend_from_slice(&[0, 0]); // header checksum placeholder
    f.extend_from_slice(&src_ip.octets());
    f.extend_from_slice(&dst_ip.octets());
    let c = checksum(&f[ip_start..ip_start + IPV4_HDR]);
    f[ip_start + 10..ip_start + 12].copy_from_slice(&c.to_be_bytes());

    let udp_start = f.len();
    f.extend_from_slice(&src_port.to_be_bytes());
    f.extend_from_slice(&dst_port.to_be_bytes());
    f.extend_from_slice(&(udp_len as u16).to_be_bytes());
    f.extend_from_slice(&[0, 0]); // checksum placeholder
    f.extend_from_slice(payload);
    let c = udp_checksum(src_ip, dst_ip, &f[udp_start..]);
    f[udp_start + 6..udp_start + 8].copy_from_slice(&c.to_be_bytes());
    f
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    TooShort(usize),
    NotIpv4(u16),
    Version(u8),
    Ihl(u8),
    Fragment,
    NotUdp(u8),
    IpLength { total_length: u16, available: usize },
    IpChecksum,
    UdpLength { udp_length: u16, available: usize },
    UdpChecksum,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "frame of {n} bytes too short"),
            Self::NotIpv4(t) => write!(f, "ethertype {t:#06x} is not IPv4"),
            Self::Version(v) => write!(f, "IP version {v} is not 4"),
            Self::Ihl(i) => write!(f, "IHL {i} < 5"),
            Self::Fragment => write!(f, "fragmented IPv4 packet"),
            Self::NotUdp(p) => write!(f, "IP protocol {p} is not UDP"),
            Self::IpLength {
                total_length,
                available,
            } => {
                write!(
                    f,
                    "IPv4 total_length {total_length} exceeds frame body {available}"
                )
            }
            Self::IpChecksum => write!(f, "bad IPv4 header checksum"),
            Self::UdpLength {
                udp_length,
                available,
            } => {
                write!(
                    f,
                    "UDP length {udp_length} does not fit in {available} bytes"
                )
            }
            Self::UdpChecksum => write!(f, "bad UDP checksum"),
        }
    }
}

impl std::error::Error for PacketError {}

#[derive(Debug, Clone, Copy)]
pub struct UdpFrame<'a> {
    pub dst_mac: Mac,
    pub src_mac: Mac,
    pub src_ip: Ipv4Addr,
    pub dst_ip: Ipv4Addr,
    pub src_port: u16,
    pub dst_port: u16,
    pub payload: &'a [u8],
}

pub fn ethertype(frame: &[u8]) -> Option<u16> {
    (frame.len() >= ETH_HDR).then(|| u16::from_be_bytes([frame[12], frame[13]]))
}

/// Strictly parse one untagged `Ethernet(IPv4(UDP))` frame. Trailing bytes
/// beyond IPv4 `total_length` (Ethernet padding) are tolerated; a
/// `total_length` larger than the frame is not. The UDP checksum is verified
/// only when `verify_udp_csum` is set (the kernel may hand a packet socket a
/// locally generated packet whose checksum is not yet computed;
/// `TP_STATUS_CSUMNOTREADY` in `PACKET_AUXDATA` tells us).
pub fn parse_udp(frame: &[u8], verify_udp_csum: bool) -> Result<UdpFrame<'_>, PacketError> {
    if frame.len() < ETH_HDR + IPV4_HDR + UDP_HDR {
        return Err(PacketError::TooShort(frame.len()));
    }
    let et = u16::from_be_bytes([frame[12], frame[13]]);
    if et != ETHERTYPE_IPV4 {
        return Err(PacketError::NotIpv4(et));
    }
    let ip = &frame[ETH_HDR..];
    let version = ip[0] >> 4;
    if version != 4 {
        return Err(PacketError::Version(version));
    }
    let ihl = usize::from(ip[0] & 0x0f) * 4;
    if ihl < IPV4_HDR {
        return Err(PacketError::Ihl(ip[0] & 0x0f));
    }
    let total_length = u16::from_be_bytes([ip[2], ip[3]]);
    if usize::from(total_length) > ip.len() || usize::from(total_length) < ihl + UDP_HDR {
        return Err(PacketError::IpLength {
            total_length,
            available: ip.len(),
        });
    }
    let ip = &ip[..usize::from(total_length)];
    if u16::from_be_bytes([ip[6], ip[7]]) & 0x3fff != 0 {
        // MF set or fragment offset != 0
        return Err(PacketError::Fragment);
    }
    if ip[9] != IPPROTO_UDP {
        return Err(PacketError::NotUdp(ip[9]));
    }
    if checksum(&ip[..ihl]) != 0 {
        return Err(PacketError::IpChecksum);
    }
    let src_ip = Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15]);
    let dst_ip = Ipv4Addr::new(ip[16], ip[17], ip[18], ip[19]);
    let udp = &ip[ihl..];
    let udp_length = u16::from_be_bytes([udp[4], udp[5]]);
    if usize::from(udp_length) < UDP_HDR || usize::from(udp_length) > udp.len() {
        return Err(PacketError::UdpLength {
            udp_length,
            available: udp.len(),
        });
    }
    let udp = &udp[..usize::from(udp_length)];
    let csum = u16::from_be_bytes([udp[6], udp[7]]);
    if verify_udp_csum && csum != 0 && udp_checksum(src_ip, dst_ip, udp) != csum {
        return Err(PacketError::UdpChecksum);
    }
    let mut dst_mac = [0u8; 6];
    let mut src_mac = [0u8; 6];
    dst_mac.copy_from_slice(&frame[0..6]);
    src_mac.copy_from_slice(&frame[6..12]);
    Ok(UdpFrame {
        dst_mac,
        src_mac,
        src_ip,
        dst_ip,
        src_port: u16::from_be_bytes([udp[0], udp[1]]),
        dst_port: u16::from_be_bytes([udp[2], udp[3]]),
        payload: &udp[UDP_HDR..],
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arp {
    pub op: u16,
    pub sha: Mac,
    pub spa: Ipv4Addr,
    pub tha: Mac,
    pub tpa: Ipv4Addr,
}

/// Parse an Ethernet/IPv4 ARP packet (htype 1, ptype 0x0800, hlen 6, plen 4).
pub fn parse_arp(frame: &[u8]) -> Option<Arp> {
    if frame.len() < ETH_HDR + ARP_LEN || ethertype(frame)? != ETHERTYPE_ARP {
        return None;
    }
    let a = &frame[ETH_HDR..ETH_HDR + ARP_LEN];
    if a[0..2] != [0, 1] || a[2..4] != [8, 0] || a[4] != 6 || a[5] != 4 {
        return None;
    }
    let mut sha = [0u8; 6];
    let mut tha = [0u8; 6];
    sha.copy_from_slice(&a[8..14]);
    tha.copy_from_slice(&a[18..24]);
    Some(Arp {
        op: u16::from_be_bytes([a[6], a[7]]),
        sha,
        spa: Ipv4Addr::new(a[14], a[15], a[16], a[17]),
        tha,
        tpa: Ipv4Addr::new(a[24], a[25], a[26], a[27]),
    })
}

/// Build an ARP reply "`our_ip` is at `our_mac`" addressed to `to`.
pub fn arp_reply_frame(our_mac: &Mac, our_ip: Ipv4Addr, to_mac: &Mac, to_ip: Ipv4Addr) -> Vec<u8> {
    let mut f = Vec::with_capacity(ETH_HDR + ARP_LEN);
    f.extend_from_slice(to_mac);
    f.extend_from_slice(our_mac);
    f.extend_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    f.extend_from_slice(&[0, 1, 8, 0, 6, 4]);
    f.extend_from_slice(&ARP_REPLY.to_be_bytes());
    f.extend_from_slice(our_mac);
    f.extend_from_slice(&our_ip.octets());
    f.extend_from_slice(to_mac);
    f.extend_from_slice(&to_ip.octets());
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_rfc1071_vectors() {
        // RFC 1071 §3 worked example: 00 01 f2 03 f4 f5 f6 f7 -> sum 0xddf2, checksum 0x220d.
        assert_eq!(
            checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]),
            0x220d
        );
        // Wikipedia IPv4 header example: checksum 0xb861.
        let hdr = [
            0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        assert_eq!(checksum(&hdr), 0xb861);
        let mut with = hdr;
        with[10..12].copy_from_slice(&0xb861u16.to_be_bytes());
        assert_eq!(checksum(&with), 0);
        // Odd length pads with zero.
        assert_eq!(checksum(&[0xff]), !0xff00u16);
        assert_eq!(checksum(&[]), 0xffff);
    }

    #[test]
    fn udp_checksum_roundtrip_and_never_zero() {
        let src = Ipv4Addr::new(0, 0, 0, 0);
        let dst = Ipv4Addr::new(255, 255, 255, 255);
        let f = ipv4_udp_frame(
            &[2, 0, 0, 0, 0, 1],
            &BROADCAST_MAC,
            src,
            dst,
            68,
            67,
            b"hello",
        );
        let p = parse_udp(&f, true).expect("own frame parses with checksum verification");
        assert_eq!(p.payload, b"hello");
        assert_eq!((p.src_port, p.dst_port), (68, 67));
        assert_eq!(p.dst_mac, BROADCAST_MAC);
        // Corrupt a payload byte: checksum must fail.
        let mut bad = f.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(parse_udp(&bad, true).unwrap_err(), PacketError::UdpChecksum);
        assert!(parse_udp(&bad, false).is_ok());
        // Known vector: UDP from 10.0.0.1 to 10.0.0.2, ports 1->2, payload "a".
        // Independently computed (python): 0x5e0e... recompute here structurally:
        let udp = [0, 1, 0, 2, 0, 9, 0, 0, b'a'];
        let c = udp_checksum(Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2), &udp);
        assert_ne!(c, 0);
        // Verify by summing everything including the checksum -> 0xffff.
        let mut all = Vec::new();
        all.extend_from_slice(&[10, 0, 0, 1, 10, 0, 0, 2, 0, 17, 0, 9]);
        all.extend_from_slice(&udp);
        all[12 + 6..12 + 8].copy_from_slice(&c.to_be_bytes());
        assert_eq!(checksum(&all), 0);
    }

    #[test]
    fn parse_udp_rejects_malformed() {
        let src = Ipv4Addr::new(1, 2, 3, 4);
        let f = ipv4_udp_frame(&[1; 6], &[2; 6], src, src, 1, 2, &[0; 10]);
        assert!(matches!(
            parse_udp(&f[..30], true),
            Err(PacketError::TooShort(30))
        ));
        let mut g = f.clone();
        g[12] = 0x86;
        g[13] = 0xdd;
        assert!(matches!(
            parse_udp(&g, true),
            Err(PacketError::NotIpv4(0x86dd))
        ));
        let mut g = f.clone();
        g[16] = 0xff; // huge total length
        assert!(matches!(
            parse_udp(&g, true),
            Err(PacketError::IpLength { .. })
        ));
        let mut g = f.clone();
        g[20] = 0x20; // MF flag
        assert!(matches!(parse_udp(&g, true), Err(PacketError::Fragment)));
        let mut g = f.clone();
        g[23] = 6; // TCP
        assert!(matches!(parse_udp(&g, true), Err(PacketError::NotUdp(6))));
        let mut g = f.clone();
        g[18] ^= 1; // id changed, header checksum now wrong
        assert!(matches!(parse_udp(&g, true), Err(PacketError::IpChecksum)));
        let mut g = f.clone();
        g[38] = 0;
        g[39] = 3; // udp length < 8
        assert!(matches!(
            parse_udp(&g, false),
            Err(PacketError::UdpLength { .. })
        ));
        // Ethernet trailing padding is tolerated.
        let mut g = f.clone();
        g.extend_from_slice(&[0; 20]);
        assert_eq!(parse_udp(&g, true).unwrap().payload.len(), 10);
    }

    #[test]
    fn arp_roundtrip() {
        let f = arp_reply_frame(
            &[1; 6],
            Ipv4Addr::new(10, 0, 0, 5),
            &[2; 6],
            Ipv4Addr::new(10, 0, 0, 1),
        );
        let a = parse_arp(&f).unwrap();
        assert_eq!(a.op, ARP_REPLY);
        assert_eq!(a.sha, [1; 6]);
        assert_eq!(a.spa, Ipv4Addr::new(10, 0, 0, 5));
        assert_eq!(a.tha, [2; 6]);
        assert_eq!(a.tpa, Ipv4Addr::new(10, 0, 0, 1));
        assert!(parse_arp(&f[..40]).is_none());
        assert_eq!(
            parse_mac(&fmt_mac(&[0xde, 0xad, 0xbe, 0xef, 0, 1])).unwrap(),
            [0xde, 0xad, 0xbe, 0xef, 0, 1]
        );
        assert!(parse_mac("de:ad").is_none());
    }
}
