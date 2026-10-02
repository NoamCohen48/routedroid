//! A minimal ICMP echo request, enough for the relay benchmark (the host
//! kernel answers it).

use std::net::Ipv4Addr;

pub fn echo_request(src: Ipv4Addr, dst: Ipv4Addr, id: u16, seq: u16) -> Vec<u8> {
    const TOTAL: u16 = 20 + 8 + 32;
    let mut p = vec![0u8; usize::from(TOTAL)];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&TOTAL.to_be_bytes());
    p[8] = 64;
    p[9] = 1;
    p[12..16].copy_from_slice(&src.octets());
    p[16..20].copy_from_slice(&dst.octets());
    let c = checksum(&p[..20]);
    p[10..12].copy_from_slice(&c.to_be_bytes());
    p[20] = 8;
    p[24..26].copy_from_slice(&id.to_be_bytes());
    p[26..28].copy_from_slice(&seq.to_be_bytes());
    for (b, i) in p[28..].iter_mut().zip(0u8..) {
        *b = i;
    }
    let c = checksum(&p[20..]);
    p[22..24].copy_from_slice(&c.to_be_bytes());
    p
}

fn checksum(d: &[u8]) -> u16 {
    let mut s = 0u32;
    for ch in d.chunks(2) {
        s += u32::from(if ch.len() == 2 {
            u16::from_be_bytes([ch[0], ch[1]])
        } else {
            u16::from(ch[0]) << 8
        });
    }
    while s >> 16 != 0 {
        s = (s & 0xffff) + (s >> 16);
    }
    !u16::try_from(s).expect("folded to 16 bits")
}

/// Whether `packet` is an echo reply (IPv4, protocol 1, type 0) for `id`.
pub fn is_echo_reply(packet: &[u8], id: u16) -> bool {
    let header = usize::from(packet.first().map_or(0, |b| b & 0x0f)) * 4;
    packet.len() >= header + 8
        && header >= 20
        && packet[9] == 1
        && packet[header] == 0
        && u16::from_be_bytes([packet[header + 4], packet[header + 5]]) == id
}
