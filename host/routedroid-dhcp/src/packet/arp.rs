//! Ethernet/IPv4 ARP (RFC 826) and the RFC 5227 probe and announcement.

use std::net::Ipv4Addr;

use super::{BROADCAST_MAC, ETH_HDR, ETHERTYPE_ARP, Mac, ethertype, mac_at};

pub const ARP_REQUEST: u16 = 1;
pub const ARP_REPLY: u16 = 2;
const ARP_LEN: usize = 28;
/// htype Ethernet, ptype IPv4, hlen 6, plen 4.
const ARP_FIXED: [u8; 6] = [0, 1, 8, 0, 6, 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arp {
    pub op: u16,
    pub sha: Mac,
    pub spa: Ipv4Addr,
    pub tha: Mac,
    pub tpa: Ipv4Addr,
}

fn ip_at(b: &[u8], at: usize) -> Ipv4Addr {
    Ipv4Addr::new(b[at], b[at + 1], b[at + 2], b[at + 3])
}

pub fn parse_arp(frame: &[u8]) -> Option<Arp> {
    if frame.len() < ETH_HDR + ARP_LEN || ethertype(frame)? != ETHERTYPE_ARP {
        return None;
    }
    let a = &frame[ETH_HDR..ETH_HDR + ARP_LEN];
    if a[..6] != ARP_FIXED {
        return None;
    }
    Some(Arp {
        op: u16::from_be_bytes([a[6], a[7]]),
        sha: mac_at(a, 8),
        spa: ip_at(a, 14),
        tha: mac_at(a, 18),
        tpa: ip_at(a, 24),
    })
}

fn frame(dst: &Mac, op: u16, sender: (&Mac, Ipv4Addr), target: (&Mac, Ipv4Addr)) -> Vec<u8> {
    let mut f = Vec::with_capacity(ETH_HDR + ARP_LEN);
    f.extend_from_slice(dst);
    f.extend_from_slice(sender.0);
    f.extend_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    f.extend_from_slice(&ARP_FIXED);
    f.extend_from_slice(&op.to_be_bytes());
    f.extend_from_slice(sender.0);
    f.extend_from_slice(&sender.1.octets());
    f.extend_from_slice(target.0);
    f.extend_from_slice(&target.1.octets());
    f
}

/// "`ip` is at `mac`", unicast to whoever asked.
pub fn reply_frame(mac: &Mac, ip: Ipv4Addr, to_mac: &Mac, to_ip: Ipv4Addr) -> Vec<u8> {
    frame(to_mac, ARP_REPLY, (mac, ip), (to_mac, to_ip))
}

/// RFC 5227 §2.1.1 probe: "who has `ip`?" from the unspecified address, so
/// nobody's cache learns anything from it.
pub fn probe_frame(mac: &Mac, ip: Ipv4Addr) -> Vec<u8> {
    let unspecified = Ipv4Addr::UNSPECIFIED;
    frame(
        &BROADCAST_MAC,
        ARP_REQUEST,
        (mac, unspecified),
        (&[0; 6], ip),
    )
}

/// RFC 5227 §2.3 announcement: a broadcast request with sender and target
/// both `ip`, which refreshes every neighbour's cache entry for it.
pub fn announce_frame(mac: &Mac, ip: Ipv4Addr) -> Vec<u8> {
    frame(&BROADCAST_MAC, ARP_REQUEST, (mac, ip), (&[0; 6], ip))
}
