//! `Ethernet(IPv4(UDP))`: the one builder and the one strict parser.

use std::net::Ipv4Addr;

use super::{
    ETH_HDR, ETHERTYPE_IPV4, IPPROTO_UDP, IPV4_HDR, Mac, PacketError, UDP_HDR, checksum, mac_at,
    udp_checksum,
};

/// IPv4 id 0, DF clear, TTL 64. `payload` is one DHCP message, far below
/// the IPv4 limit; a larger one is a programming error.
pub fn ipv4_udp_frame(
    src_mac: &Mac,
    dst_mac: &Mac,
    (src_ip, dst_ip): (Ipv4Addr, Ipv4Addr),
    (src_port, dst_port): (u16, u16),
    payload: &[u8],
) -> Vec<u8> {
    let ip_len =
        u16::try_from(IPV4_HDR + UDP_HDR + payload.len()).expect("payload fits one IPv4 packet");
    let udp_len = ip_len - 20; // less the IPv4 header
    let mut f = Vec::with_capacity(ETH_HDR + usize::from(ip_len));
    f.extend_from_slice(dst_mac);
    f.extend_from_slice(src_mac);
    f.extend_from_slice(&ETHERTYPE_IPV4.to_be_bytes());

    f.extend_from_slice(&[0x45, 0x00]); // version 4, IHL 5; DSCP/ECN
    f.extend_from_slice(&ip_len.to_be_bytes());
    f.extend_from_slice(&[0, 0, 0, 0, 64, IPPROTO_UDP, 0, 0]); // id, frag, TTL, proto, csum
    f.extend_from_slice(&src_ip.octets());
    f.extend_from_slice(&dst_ip.octets());
    let c = checksum(&f[ETH_HDR..ETH_HDR + IPV4_HDR]);
    f[ETH_HDR + 10..ETH_HDR + 12].copy_from_slice(&c.to_be_bytes());

    let udp = f.len();
    f.extend_from_slice(&src_port.to_be_bytes());
    f.extend_from_slice(&dst_port.to_be_bytes());
    f.extend_from_slice(&udp_len.to_be_bytes());
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(payload);
    let c = udp_checksum(src_ip, dst_ip, &f[udp..]);
    f[udp + 6..udp + 8].copy_from_slice(&c.to_be_bytes());
    f
}

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

/// Strictly parse one untagged frame. Ethernet padding past IPv4
/// `total_length` is tolerated; a `total_length` past the frame is not. The
/// UDP checksum is verified only with `verify_udp_csum`: the kernel may hand
/// a packet socket a local packet whose checksum offload is still pending
/// (`TP_STATUS_CSUMNOTREADY`).
pub fn parse_udp(frame: &[u8], verify_udp_csum: bool) -> Result<UdpFrame<'_>, PacketError> {
    if frame.len() < ETH_HDR + IPV4_HDR + UDP_HDR {
        return Err(PacketError::TooShort(frame.len()));
    }
    let et = u16::from_be_bytes([frame[12], frame[13]]);
    if et != ETHERTYPE_IPV4 {
        return Err(PacketError::NotIpv4(et));
    }
    let ip = &frame[ETH_HDR..];
    if ip[0] >> 4 != 4 {
        return Err(PacketError::Version(ip[0] >> 4));
    }
    let ihl = usize::from(ip[0] & 0x0f) * 4;
    if ihl < IPV4_HDR {
        return Err(PacketError::Ihl(ip[0] & 0x0f));
    }
    let total_length = u16::from_be_bytes([ip[2], ip[3]]);
    let total = usize::from(total_length);
    if total > ip.len() || total < ihl + UDP_HDR {
        return Err(PacketError::IpLength {
            total_length,
            available: ip.len(),
        });
    }
    let ip = &ip[..total];
    if u16::from_be_bytes([ip[6], ip[7]]) & 0x3fff != 0 {
        return Err(PacketError::Fragment); // MF set or a non-zero offset
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
    Ok(UdpFrame {
        dst_mac: mac_at(frame, 0),
        src_mac: mac_at(frame, 6),
        src_ip,
        dst_ip,
        src_port: u16::from_be_bytes([udp[0], udp[1]]),
        dst_port: u16::from_be_bytes([udp[2], udp[3]]),
        payload: &udp[UDP_HDR..],
    })
}
