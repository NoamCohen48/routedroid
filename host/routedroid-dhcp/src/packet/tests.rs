use std::net::Ipv4Addr;

use super::*;

const ANY: Ipv4Addr = Ipv4Addr::UNSPECIFIED;

#[test]
fn checksum_rfc1071_vectors() {
    // RFC 1071 §3 worked example: sum 0xddf2, checksum 0x220d.
    assert_eq!(
        checksum(&[0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7]),
        0x220d
    );
    // Wikipedia's IPv4 header example: checksum 0xb861.
    let hdr = [
        0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    assert_eq!(checksum(&hdr), 0xb861);
    let mut with = hdr;
    with[10..12].copy_from_slice(&0xb861u16.to_be_bytes());
    assert_eq!(checksum(&with), 0);
    assert_eq!(checksum(&[0xff]), !0xff00u16, "odd length pads with zero");
    assert_eq!(checksum(&[]), 0xffff);
}

#[test]
fn udp_checksum_roundtrip_and_never_zero() {
    let f = ipv4_udp_frame(
        &[2, 0, 0, 0, 0, 1],
        &BROADCAST_MAC,
        (ANY, Ipv4Addr::BROADCAST),
        (68, 67),
        b"hello",
    );
    let p = parse_udp(&f, true).expect("own frame parses with checksum verification");
    assert_eq!(p.payload, b"hello");
    assert_eq!((p.src_port, p.dst_port), (68, 67));
    assert_eq!(p.dst_mac, BROADCAST_MAC);
    let mut bad = f.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert_eq!(parse_udp(&bad, true).unwrap_err(), PacketError::UdpChecksum);
    assert!(parse_udp(&bad, false).is_ok());
    // Summing everything, the checksum included, gives zero.
    let udp = [0, 1, 0, 2, 0, 9, 0, 0, b'a'];
    let c = udp_checksum(Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2), &udp);
    assert_ne!(c, 0);
    let mut all = vec![10, 0, 0, 1, 10, 0, 0, 2, 0, 17, 0, 9];
    all.extend_from_slice(&udp);
    all[18..20].copy_from_slice(&c.to_be_bytes());
    assert_eq!(checksum(&all), 0);
}

#[test]
fn parse_udp_rejects_malformed() {
    let src = Ipv4Addr::new(1, 2, 3, 4);
    let f = ipv4_udp_frame(&[1; 6], &[2; 6], (src, src), (1, 2), &[0; 10]);
    assert_eq!(
        parse_udp(&f[..30], true).unwrap_err(),
        PacketError::TooShort(30)
    );
    let corrupt = |at: usize, v: u8| {
        let mut g = f.clone();
        g[at] = v;
        g
    };
    let mut ipv6 = corrupt(12, 0x86);
    ipv6[13] = 0xdd;
    assert_eq!(
        parse_udp(&ipv6, true).unwrap_err(),
        PacketError::NotIpv4(0x86dd)
    );
    assert!(matches!(
        parse_udp(&corrupt(16, 0xff), true),
        Err(PacketError::IpLength { .. })
    ));
    assert_eq!(
        parse_udp(&corrupt(20, 0x20), true).unwrap_err(),
        PacketError::Fragment
    );
    assert_eq!(
        parse_udp(&corrupt(23, 6), true).unwrap_err(),
        PacketError::NotUdp(6)
    );
    assert_eq!(
        parse_udp(&corrupt(18, 1), true).unwrap_err(),
        PacketError::IpChecksum
    );
    let mut short_udp = corrupt(38, 0);
    short_udp[39] = 3;
    assert!(matches!(
        parse_udp(&short_udp, false),
        Err(PacketError::UdpLength { .. })
    ));
    let mut padded = f.clone();
    padded.extend_from_slice(&[0; 20]);
    assert_eq!(
        parse_udp(&padded, true).unwrap().payload.len(),
        10,
        "Ethernet padding"
    );
}

#[test]
fn arp_frames_roundtrip() {
    let me = [1; 6];
    let ip = Ipv4Addr::new(10, 0, 0, 5);
    let a = parse_arp(&reply_frame(&me, ip, &[2; 6], Ipv4Addr::new(10, 0, 0, 1))).unwrap();
    assert_eq!(
        a,
        Arp {
            op: ARP_REPLY,
            sha: me,
            spa: ip,
            tha: [2; 6],
            tpa: Ipv4Addr::new(10, 0, 0, 1)
        }
    );
    let probe = probe_frame(&me, ip);
    assert_eq!(probe[..6], BROADCAST_MAC);
    let p = parse_arp(&probe).unwrap();
    assert_eq!((p.op, p.spa, p.tpa, p.tha), (ARP_REQUEST, ANY, ip, [0; 6]));
    let n = parse_arp(&announce_frame(&me, ip)).unwrap();
    assert_eq!((n.op, n.sha, n.spa, n.tpa), (ARP_REQUEST, me, ip, ip));
    assert!(parse_arp(&probe[..40]).is_none());
}

#[test]
fn macs_format_and_parse() {
    let m = [0xde, 0xad, 0xbe, 0xef, 0, 1];
    assert_eq!(fmt_mac(&m), "de:ad:be:ef:00:01");
    assert_eq!(parse_mac(&fmt_mac(&m)), Some(m));
    for bad in [
        "de:ad",
        "de:ad:be:ef:00:01:02",
        "de:ad:be:ef:00:1",
        "zz:ad:be:ef:00:01",
    ] {
        assert_eq!(parse_mac(bad), None, "{bad}");
    }
}
