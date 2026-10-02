//! One complete DISCOVER frame, byte for byte, against bytes computed by an
//! independent Python implementation of RFC 791/768/2131.

use std::net::Ipv4Addr;

use crate::dhcp::{self, CLIENT_PORT, Identity, MessageType, SERVER_PORT};
use crate::identity::ClientId;
use crate::packet::{self, BROADCAST_MAC};

/// MAC 02:00:00:00:00:01, device 0123456789abcdef, XID 0x12345678, secs 3.
fn discover_frame() -> Vec<u8> {
    let mac = [0x02, 0, 0, 0, 0, 0x01];
    let device = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
    let id = Identity {
        mac,
        client_id: ClientId::new(&device, &mac),
    };
    let msg = dhcp::discover(&id, 0x1234_5678, 3);
    let ips = (Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST);
    packet::ipv4_udp_frame(
        &mac,
        &BROADCAST_MAC,
        ips,
        (CLIENT_PORT, SERVER_PORT),
        &msg.encode(),
    )
}

/// Ethernet broadcast; IPv4 0.0.0.0 -> 255.255.255.255, id 0, TTL 64,
/// checksum 0x79a5; UDP 68 -> 67, length 309, checksum 0x4bad; BOOTP op 1,
/// htype 1, hlen 6, xid 0x12345678, secs 3, flags 0x8000, chaddr the MAC;
/// options 53=1, 61=00"routedroid:0123456789abcdef:020000000001",
/// 55=1,3,6,51,54,58,59,121, 57=1500, END. Over 300 bytes, so no padding.
const DISCOVER_GOLDEN: &[&str] = &[
    "ffffffffffff02000000000108004500014900000000401179a500000000ffff",
    "ffff0044004301354bad01010600123456780003800000000000000000000000",
    "0000000000000200000000010000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "00000000000000000000000000000000000000000000638253633501013d2900",
    "726f75746564726f69643a303132333435363738396162636465663a30323030",
    "3030303030303031370801030633363a3b79390205dcff",
];

#[test]
fn discover_frame_golden_bytes() {
    let hex = DISCOVER_GOLDEN.concat();
    let expected: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let got = discover_frame();
    assert_eq!(got.len(), 343);
    if got != expected {
        let first = got.iter().zip(&expected).position(|(a, b)| a != b);
        panic!("DISCOVER frame differs from golden at byte {first:?}");
    }
    // And it round-trips through our own strict parsers.
    let udp = packet::parse_udp(&got, true).unwrap();
    let msg = dhcp::Message::parse(udp.payload).unwrap();
    assert_eq!(msg.xid, 0x1234_5678);
    assert_eq!(msg.message_type(), Some(MessageType::Discover));
}
