//! A minimal ICMP echo request, enough for the relay benchmark (the host
//! kernel answers it).

use std::net::Ipv4Addr;

pub fn echo_request(src: Ipv4Addr, dst: Ipv4Addr, id: u16, seq: u16) -> Vec<u8> {
    let total = 20 + 8 + 32;
    let mut p = vec![0u8; total];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    p[8] = 64;
    p[9] = 1;
    p[12..16].copy_from_slice(&src.octets());
    p[16..20].copy_from_slice(&dst.octets());
    let c = checksum(&p[..20]);
    p[10..12].copy_from_slice(&c.to_be_bytes());
    p[20] = 8;
    p[24..26].copy_from_slice(&id.to_be_bytes());
    p[26..28].copy_from_slice(&seq.to_be_bytes());
    for (i, b) in p[28..].iter_mut().enumerate() {
        *b = i as u8;
    }
    let c = checksum(&p[20..]);
    p[22..24].copy_from_slice(&c.to_be_bytes());
    p
}

fn checksum(d: &[u8]) -> u16 {
    let mut s = 0u32;
    for ch in d.chunks(2) {
        s += u32::from(if ch.len() == 2 { u16::from_be_bytes([ch[0], ch[1]]) } else { u16::from(ch[0]) << 8 });
    }
    while s >> 16 != 0 {
        s = (s & 0xffff) + (s >> 16);
    }
    !(s as u16)
}
