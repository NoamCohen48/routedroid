use std::net::Ipv4Addr;

use super::*;
use crate::identity::ClientId;
use crate::packet::Mac;

fn identity(mac: Mac) -> Identity {
    Identity {
        mac,
        client_id: ClientId::new(&[0xab; 8], &mac),
    }
}

/// Tiny deterministic PRNG (xorshift64*) so the fuzz loop is reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn byte(&mut self) -> u8 {
        self.next().to_be_bytes()[0]
    }
    /// Uniform enough in `0..n`.
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
}

#[test]
fn parsers_never_panic_on_random_input() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let id = identity([2, 0, 0, 0, 0, 1]);
    let template = discover(&id, 1, 0).encode();
    for round in 0..20_000 {
        let len = rng.below(600);
        let mut buf: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
        if round % 2 == 1 {
            // Half the rounds: start from a valid message and corrupt it,
            // so the option walker actually gets exercised past the cookie.
            buf = template.clone();
            let flips = 1 + rng.below(12);
            for _ in 0..flips {
                let i = rng.below(buf.len());
                buf[i] = rng.byte();
            }
            let cut = rng.below(buf.len() + 1);
            buf.truncate(cut.max(1));
        }
        if let Ok(m) = Message::parse(&buf) {
            let o = Options::from_message(&m);
            let _ = m.message_type();
            let _ = m.chaddr_mac();
            let _ = format!("{o:?}");
            let _ = m.encode();
        }
        let _ = parse_classless_routes(&buf);
        let mut sink = Vec::new();
        let _ = parse_options(&buf, &mut sink);
    }
}

#[test]
fn option_walker_errors_are_precise() {
    let mut out = Vec::new();
    assert_eq!(
        parse_options(&[53], &mut out),
        Err(ParseError::DanglingCode { code: 53, at: 0 })
    );
    assert_eq!(
        parse_options(&[0, 0, 53, 5, 1], &mut out),
        Err(ParseError::TruncatedOption { code: 53, at: 2 })
    );
    out.clear();
    assert_eq!(parse_options(&[0, 53, 1, 2, 255, 99, 99], &mut out), Ok(()));
    assert_eq!(out, vec![(53, vec![2])]);
    assert_eq!(
        Message::parse(&[0; 100]).unwrap_err(),
        ParseError::TooShort(100)
    );
    assert_eq!(
        Message::parse(&[0; 240]).unwrap_err(),
        ParseError::BadMagic([0; 4])
    );
}

#[test]
fn encode_parse_roundtrip_and_overload() {
    let id = identity([0xde, 0xad, 0xbe, 0xef, 0, 1]);
    let m = request_selecting(
        &id,
        0xdead_beef,
        7,
        Ipv4Addr::new(10, 1, 2, 3),
        Ipv4Addr::new(10, 1, 2, 1),
    );
    let enc = m.encode();
    assert_eq!(
        enc.len(),
        313,
        "240 fixed + 73 of options, past the 300 minimum"
    );
    let short = release(
        &id,
        1,
        Ipv4Addr::new(10, 1, 2, 3),
        Ipv4Addr::new(10, 1, 2, 1),
    );
    assert_eq!(
        short.encode().len(),
        300,
        "zero-padded to the BOOTP minimum"
    );
    let back = Message::parse(&enc).unwrap();
    assert_eq!(back, m);
    let o = Options::from_message(&back);
    assert_eq!(o.message_type, Some(MessageType::Request));
    assert_eq!(o.requested_ip, Some(Ipv4Addr::new(10, 1, 2, 3)));
    assert_eq!(o.server_id, Some(Ipv4Addr::new(10, 1, 2, 1)));
    assert!(o.malformed.is_empty());

    // Overload 3: options continue in `file` (offset 108) and `sname` (44).
    let mut raw = vec![0u8; 240];
    raw[0] = BOOTREPLY;
    raw[236..240].copy_from_slice(&MAGIC_COOKIE);
    raw.extend_from_slice(&[52, 1, 3, 255]);
    raw[108..113].copy_from_slice(&[53, 1, 5, 255, 0]);
    raw[44..51].copy_from_slice(&[51, 4, 0, 0, 0x0e, 0x10, 255]);
    let m = Message::parse(&raw).unwrap();
    let o = Options::from_message(&m);
    assert_eq!(o.message_type, Some(MessageType::Ack));
    assert_eq!(o.lease_secs, Some(3600));

    // RFC 3396: split option bodies concatenate; malformed sizes are flagged.
    let mut raw = vec![0u8; 240];
    raw[236..240].copy_from_slice(&MAGIC_COOKIE);
    raw.extend_from_slice(&[
        6, 4, 8, 8, 8, 8, 6, 4, 1, 1, 1, 1, 1, 3, 1, 2, 3, 58, 2, 0, 1, 255,
    ]);
    let o = Options::from_message(&Message::parse(&raw).unwrap());
    assert_eq!(
        o.dns,
        vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(1, 1, 1, 1)]
    );
    assert_eq!(o.subnet_mask, None);
    assert_eq!(o.t1, None);
    assert_eq!(o.malformed, vec![1, 58]);
}

#[test]
fn classless_routes_rfc3442_examples() {
    // RFC 3442 §9 style: 10.0.0.0/8 via 10.17.0.1, 10.229.0.0/16 via 10.229.0.1,
    // 0.0.0.0/0 via 10.27.129.1, 10.27.129.0/24 via 10.27.129.1.
    let b = [
        8, 10, 10, 17, 0, 1, 16, 10, 229, 10, 229, 0, 1, 0, 10, 27, 129, 1, 24, 10, 27, 129, 10,
        27, 129, 1,
    ];
    let (r, ok) = parse_classless_routes(&b);
    assert!(ok);
    assert_eq!(
        r,
        vec![
            StaticRoute {
                dest: "10.0.0.0".parse().unwrap(),
                prefix: 8,
                router: "10.17.0.1".parse().unwrap()
            },
            StaticRoute {
                dest: "10.229.0.0".parse().unwrap(),
                prefix: 16,
                router: "10.229.0.1".parse().unwrap()
            },
            StaticRoute {
                dest: "0.0.0.0".parse().unwrap(),
                prefix: 0,
                router: "10.27.129.1".parse().unwrap()
            },
            StaticRoute {
                dest: "10.27.129.0".parse().unwrap(),
                prefix: 24,
                router: "10.27.129.1".parse().unwrap()
            },
        ]
    );
    // /25 needs 4 significant octets; host bits are masked.
    let (r, ok) = parse_classless_routes(&[25, 192, 168, 1, 255, 192, 168, 1, 1]);
    assert!(ok);
    assert_eq!(r[0].dest, Ipv4Addr::new(192, 168, 1, 128));
    // Truncated tail and prefix > 32 are flagged, prior routes kept.
    let (r, ok) = parse_classless_routes(&[8, 10, 10, 17, 0, 1, 24, 10, 27]);
    assert!(!ok);
    assert_eq!(r.len(), 1);
    let (r, ok) = parse_classless_routes(&[33, 1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(!ok);
    assert!(r.is_empty());
    assert_eq!(parse_classless_routes(&[]), (vec![], true));
}

#[test]
fn builders_follow_the_state_table() {
    let id = identity([2, 0, 0, 0, 0, 1]);
    let lease = Ipv4Addr::new(192, 168, 50, 100);
    let server = Ipv4Addr::new(192, 168, 50, 1);
    let d = discover(&id, 1, 0);
    assert_eq!(d.flags, FLAG_BROADCAST);
    assert!(d.ciaddr.is_unspecified());
    assert_eq!(d.option(opt::CLIENT_ID).unwrap(), id.client_id.option());
    assert_eq!(d.option(opt::PARAM_REQUEST).unwrap(), PARAM_REQUEST_LIST);
    assert_eq!(d.option(opt::MAX_MESSAGE_SIZE).unwrap(), [5, 220]);
    let r = request_init_reboot(&id, 1, 0, lease);
    assert!(r.option(opt::REQUESTED_IP).is_some() && r.option(opt::SERVER_ID).is_none());
    let r = request_renew(&id, 1, 0, lease);
    assert_eq!(r.ciaddr, lease);
    assert_eq!(r.flags, 0);
    assert!(r.option(opt::REQUESTED_IP).is_none() && r.option(opt::SERVER_ID).is_none());
    let r = release(&id, 1, lease, server);
    assert_eq!(r.ciaddr, lease);
    assert_eq!(r.option(opt::SERVER_ID).unwrap(), server.octets());
    assert!(r.option(opt::PARAM_REQUEST).is_none());
    let r = decline(&id, 1, lease, server);
    assert_eq!((r.ciaddr, r.flags), (Ipv4Addr::UNSPECIFIED, 0));
    assert_eq!(r.message_type(), Some(MessageType::Decline));
    assert_eq!(r.option(opt::REQUESTED_IP).unwrap(), lease.octets());
    assert_eq!(r.option(opt::SERVER_ID).unwrap(), server.octets());
    assert_eq!(r.option(opt::CLIENT_ID).unwrap(), id.client_id.option());
}

#[test]
fn message_types_number_one_to_eight() {
    for v in 1..=8 {
        let t = MessageType::from_u8(v).unwrap();
        assert_eq!(t.as_u8(), v);
    }
    assert_eq!(MessageType::from_u8(0), None);
    assert_eq!(MessageType::from_u8(9), None);
    assert_eq!(MessageType::Nak.to_string(), "NAK");
}

#[test]
fn an_echoed_client_id_is_decoded() {
    let id = identity([2, 0, 0, 0, 0, 1]);
    let o = Options::from_message(&discover(&id, 1, 0));
    assert_eq!(o.client_id, Some(id.client_id.option()));
}
