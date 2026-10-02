//! BOOTP/DHCP message codec (RFC 2131/2132/3396/3442) and the message
//! builders for every state in architecture.md §6.1. Pure functions.

use std::fmt;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::packet::Mac;

pub const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
pub const FLAG_BROADCAST: u16 = 0x8000;
pub const CLIENT_PORT: u16 = 68;
pub const SERVER_PORT: u16 = 67;
pub const BOOTREQUEST: u8 = 1;
pub const BOOTREPLY: u8 = 2;
pub const HTYPE_ETHERNET: u8 = 1;
/// Fixed BOOTP header up to and including `file`.
const FIXED_LEN: usize = 236;
/// Minimum BOOTP payload; some relays and servers drop shorter datagrams.
const MIN_PAYLOAD: usize = 300;
/// Option 57 value we advertise (RFC 2132 §9.10 counts the IP+UDP headers).
pub const MAX_MESSAGE_SIZE: u16 = 1500;
/// Option 55 list, in this order.
pub const PARAM_REQUEST_LIST: [u8; 8] = [1, 3, 6, 51, 54, 58, 59, 121];

pub mod opt {
    pub const PAD: u8 = 0;
    pub const SUBNET_MASK: u8 = 1;
    pub const ROUTER: u8 = 3;
    pub const DNS: u8 = 6;
    pub const REQUESTED_IP: u8 = 50;
    pub const LEASE_TIME: u8 = 51;
    pub const OVERLOAD: u8 = 52;
    pub const MESSAGE_TYPE: u8 = 53;
    pub const SERVER_ID: u8 = 54;
    pub const PARAM_REQUEST: u8 = 55;
    pub const MESSAGE: u8 = 56;
    pub const MAX_MESSAGE_SIZE: u8 = 57;
    pub const T1: u8 = 58;
    pub const T2: u8 = 59;
    pub const CLIENT_ID: u8 = 61;
    pub const CLASSLESS_ROUTES: u8 = 121;
    pub const END: u8 = 255;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Discover,
    Offer,
    Request,
    Decline,
    Ack,
    Nak,
    Release,
    Inform,
}

impl MessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Discover,
            2 => Self::Offer,
            3 => Self::Request,
            4 => Self::Decline,
            5 => Self::Ack,
            6 => Self::Nak,
            7 => Self::Release,
            8 => Self::Inform,
            _ => return None,
        })
    }
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Discover => 1,
            Self::Offer => 2,
            Self::Request => 3,
            Self::Decline => 4,
            Self::Ack => 5,
            Self::Nak => 6,
            Self::Release => 7,
            Self::Inform => 8,
        }
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Discover => "DISCOVER",
            Self::Offer => "OFFER",
            Self::Request => "REQUEST",
            Self::Decline => "DECLINE",
            Self::Ack => "ACK",
            Self::Nak => "NAK",
            Self::Release => "RELEASE",
            Self::Inform => "INFORM",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub op: u8,
    pub htype: u8,
    pub hlen: u8,
    pub hops: u8,
    pub xid: u32,
    pub secs: u16,
    pub flags: u16,
    pub ciaddr: Ipv4Addr,
    pub yiaddr: Ipv4Addr,
    pub siaddr: Ipv4Addr,
    pub giaddr: Ipv4Addr,
    pub chaddr: [u8; 16],
    /// Options in wire order, duplicates preserved (RFC 3396 concatenation
    /// happens in [`Message::option`]).
    pub options: Vec<(u8, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    TooShort(usize),
    BadMagic([u8; 4]),
    /// An option's length byte runs past the end of the buffer.
    TruncatedOption {
        code: u8,
        at: usize,
    },
    /// Option code without a length byte.
    DanglingCode {
        code: u8,
        at: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "DHCP payload of {n} bytes shorter than 240"),
            Self::BadMagic(m) => write!(f, "bad magic cookie {m:02x?}"),
            Self::TruncatedOption { code, at } => {
                write!(f, "option {code} at offset {at} is truncated")
            }
            Self::DanglingCode { code, at } => {
                write!(f, "option {code} at offset {at} has no length byte")
            }
        }
    }
}

impl std::error::Error for ParseError {}

fn ip4(b: &[u8]) -> Ipv4Addr {
    Ipv4Addr::new(b[0], b[1], b[2], b[3])
}

/// Parse a TLV option block. PAD is skipped, END stops; anything else must
/// carry a complete length + body. Never panics.
pub fn parse_options(buf: &[u8], out: &mut Vec<(u8, Vec<u8>)>) -> Result<(), ParseError> {
    let mut i = 0;
    while i < buf.len() {
        let code = buf[i];
        match code {
            opt::PAD => i += 1,
            opt::END => return Ok(()),
            _ => {
                let Some(&len) = buf.get(i + 1) else {
                    return Err(ParseError::DanglingCode { code, at: i });
                };
                let start = i + 2;
                let end = start + usize::from(len);
                let Some(body) = buf.get(start..end) else {
                    return Err(ParseError::TruncatedOption { code, at: i });
                };
                out.push((code, body.to_vec()));
                i = end;
            }
        }
    }
    Ok(())
}

impl Message {
    /// Strict decode: fixed header, magic cookie, well-formed options,
    /// option-overload (52) into `file`/`sname` honoured.
    pub fn parse(buf: &[u8]) -> Result<Self, ParseError> {
        if buf.len() < FIXED_LEN + 4 {
            return Err(ParseError::TooShort(buf.len()));
        }
        let magic = [buf[236], buf[237], buf[238], buf[239]];
        if magic != MAGIC_COOKIE {
            return Err(ParseError::BadMagic(magic));
        }
        let mut options = Vec::new();
        parse_options(&buf[240..], &mut options)?;
        let overload = options
            .iter()
            .find(|(c, _)| *c == opt::OVERLOAD)
            .and_then(|(_, v)| (v.len() == 1).then(|| v[0]))
            .unwrap_or(0);
        if overload & 1 != 0 {
            parse_options(&buf[108..236], &mut options)?;
        }
        if overload & 2 != 0 {
            parse_options(&buf[44..108], &mut options)?;
        }
        let mut chaddr = [0u8; 16];
        chaddr.copy_from_slice(&buf[28..44]);
        Ok(Self {
            op: buf[0],
            htype: buf[1],
            hlen: buf[2],
            hops: buf[3],
            xid: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            secs: u16::from_be_bytes([buf[8], buf[9]]),
            flags: u16::from_be_bytes([buf[10], buf[11]]),
            ciaddr: ip4(&buf[12..16]),
            yiaddr: ip4(&buf[16..20]),
            siaddr: ip4(&buf[20..24]),
            giaddr: ip4(&buf[24..28]),
            chaddr,
            options,
        })
    }

    /// Encode with `sname`/`file` zeroed, END terminator, zero-padded to 300 bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(MIN_PAYLOAD);
        b.extend_from_slice(&[self.op, self.htype, self.hlen, self.hops]);
        b.extend_from_slice(&self.xid.to_be_bytes());
        b.extend_from_slice(&self.secs.to_be_bytes());
        b.extend_from_slice(&self.flags.to_be_bytes());
        b.extend_from_slice(&self.ciaddr.octets());
        b.extend_from_slice(&self.yiaddr.octets());
        b.extend_from_slice(&self.siaddr.octets());
        b.extend_from_slice(&self.giaddr.octets());
        b.extend_from_slice(&self.chaddr);
        b.resize(FIXED_LEN, 0); // sname + file
        b.extend_from_slice(&MAGIC_COOKIE);
        for (code, body) in &self.options {
            assert!(body.len() <= 255, "option {code} body too long");
            b.push(*code);
            b.push(body.len() as u8);
            b.extend_from_slice(body);
        }
        b.push(opt::END);
        if b.len() < MIN_PAYLOAD {
            b.resize(MIN_PAYLOAD, 0);
        }
        b
    }

    /// Concatenated body of every instance of `code` (RFC 3396), `None` if absent.
    pub fn option(&self, code: u8) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut found = false;
        for (c, body) in &self.options {
            if *c == code {
                found = true;
                out.extend_from_slice(body);
            }
        }
        found.then_some(out)
    }

    pub fn message_type(&self) -> Option<MessageType> {
        let v = self.option(opt::MESSAGE_TYPE)?;
        (v.len() == 1).then(|| MessageType::from_u8(v[0])).flatten()
    }

    pub fn chaddr_mac(&self) -> Mac {
        let mut m = [0u8; 6];
        m.copy_from_slice(&self.chaddr[..6]);
        m
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticRoute {
    pub dest: Ipv4Addr,
    pub prefix: u8,
    pub router: Ipv4Addr,
}

impl fmt::Display for StaticRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} via {}", self.dest, self.prefix, self.router)
    }
}

/// RFC 3442 classless static route list. Returns the routes decoded before
/// the first malformed entry plus whether the whole option was well-formed.
pub fn parse_classless_routes(b: &[u8]) -> (Vec<StaticRoute>, bool) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let prefix = b[i];
        if prefix > 32 {
            return (out, false);
        }
        let n = usize::from(prefix).div_ceil(8);
        let Some(entry) = b.get(i + 1..i + 1 + n + 4) else {
            return (out, false);
        };
        let mut dest = [0u8; 4];
        dest[..n].copy_from_slice(&entry[..n]);
        // Mask host bits so the destination is canonical.
        let mask: u32 = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - u32::from(prefix))
        };
        let dest = Ipv4Addr::from(u32::from_be_bytes(dest) & mask);
        out.push(StaticRoute {
            dest,
            prefix,
            router: ip4(&entry[n..n + 4]),
        });
        i += 1 + n + 4;
    }
    (out, true)
}

/// The options we care about, decoded leniently: a malformed individual
/// option is reported in `malformed` and otherwise ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub message_type: Option<MessageType>,
    pub subnet_mask: Option<Ipv4Addr>,
    pub routers: Vec<Ipv4Addr>,
    pub dns: Vec<Ipv4Addr>,
    pub server_id: Option<Ipv4Addr>,
    pub requested_ip: Option<Ipv4Addr>,
    pub lease_secs: Option<u32>,
    pub t1: Option<u32>,
    pub t2: Option<u32>,
    pub classless_routes: Vec<StaticRoute>,
    pub message: Option<String>,
    pub malformed: Vec<u8>,
}

fn ip_list(b: &[u8]) -> Option<Vec<Ipv4Addr>> {
    (!b.is_empty() && b.len().is_multiple_of(4))
        .then(|| b.as_chunks::<4>().0.iter().map(|c| ip4(c)).collect())
}

fn u32_opt(b: &[u8]) -> Option<u32> {
    (b.len() == 4).then(|| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Options {
    pub fn from_message(m: &Message) -> Self {
        let mut o = Self::default();
        let get = |code: u8, o: &mut Self| -> Option<Vec<u8>> {
            let v = m.option(code)?;
            if v.is_empty() {
                o.malformed.push(code);
                return None;
            }
            Some(v)
        };
        macro_rules! take {
            ($code:expr, $parse:expr) => {
                if let Some(v) = get($code, &mut o) {
                    match $parse(&v[..]) {
                        Some(x) => Some(x),
                        None => {
                            o.malformed.push($code);
                            None
                        }
                    }
                } else {
                    None
                }
            };
        }
        o.message_type = take!(opt::MESSAGE_TYPE, |b: &[u8]| (b.len() == 1)
            .then(|| MessageType::from_u8(b[0]))
            .flatten());
        o.subnet_mask = take!(opt::SUBNET_MASK, |b: &[u8]| (b.len() == 4).then(|| ip4(b)));
        o.routers = take!(opt::ROUTER, ip_list).unwrap_or_default();
        o.dns = take!(opt::DNS, ip_list).unwrap_or_default();
        o.server_id = take!(opt::SERVER_ID, |b: &[u8]| (b.len() == 4).then(|| ip4(b)));
        o.requested_ip = take!(opt::REQUESTED_IP, |b: &[u8]| (b.len() == 4).then(|| ip4(b)));
        o.lease_secs = take!(opt::LEASE_TIME, u32_opt);
        o.t1 = take!(opt::T1, u32_opt);
        o.t2 = take!(opt::T2, u32_opt);
        o.message = take!(opt::MESSAGE, |b: &[u8]| Some(
            String::from_utf8_lossy(b).into_owned()
        ));
        if let Some(v) = get(opt::CLASSLESS_ROUTES, &mut o) {
            let (routes, ok) = parse_classless_routes(&v);
            if !ok {
                o.malformed.push(opt::CLASSLESS_ROUTES);
            }
            o.classless_routes = routes;
        }
        o
    }
}

/// Who we are on the wire: interface MAC in `chaddr` and the option 61 body
/// (type byte 0 followed by the client-id string).
#[derive(Debug, Clone)]
pub struct Identity {
    pub mac: Mac,
    pub client_id: Vec<u8>,
}

impl Identity {
    pub fn new(mac: Mac, client_id: &str) -> Self {
        let mut v = Vec::with_capacity(client_id.len() + 1);
        v.push(0);
        v.extend_from_slice(client_id.as_bytes());
        assert!(v.len() <= 255, "client id too long for one option");
        Self { mac, client_id: v }
    }
}

fn base(id: &Identity, xid: u32, secs: u16, mtype: MessageType, ciaddr: Ipv4Addr) -> Message {
    let mut chaddr = [0u8; 16];
    chaddr[..6].copy_from_slice(&id.mac);
    // The broadcast flag only means something while we have no address the
    // server could unicast to (RFC 2131 §4.1).
    let flags = if ciaddr.is_unspecified() {
        FLAG_BROADCAST
    } else {
        0
    };
    Message {
        op: BOOTREQUEST,
        htype: HTYPE_ETHERNET,
        hlen: 6,
        hops: 0,
        xid,
        secs,
        flags,
        ciaddr,
        yiaddr: Ipv4Addr::UNSPECIFIED,
        siaddr: Ipv4Addr::UNSPECIFIED,
        giaddr: Ipv4Addr::UNSPECIFIED,
        chaddr,
        options: vec![
            (opt::MESSAGE_TYPE, vec![mtype.as_u8()]),
            (opt::CLIENT_ID, id.client_id.clone()),
        ],
    }
}

fn push_common_tail(m: &mut Message) {
    m.options
        .push((opt::PARAM_REQUEST, PARAM_REQUEST_LIST.to_vec()));
    m.options.push((
        opt::MAX_MESSAGE_SIZE,
        MAX_MESSAGE_SIZE.to_be_bytes().to_vec(),
    ));
}

/// DISCOVER: broadcast, ciaddr 0, option 61, broadcast flag.
pub fn discover(id: &Identity, xid: u32, secs: u16) -> Message {
    let mut m = base(id, xid, secs, MessageType::Discover, Ipv4Addr::UNSPECIFIED);
    push_common_tail(&mut m);
    m
}

/// SELECTING REQUEST: broadcast, requested address (50) + server id (54).
pub fn request_selecting(
    id: &Identity,
    xid: u32,
    secs: u16,
    requested: Ipv4Addr,
    server: Ipv4Addr,
) -> Message {
    let mut m = base(id, xid, secs, MessageType::Request, Ipv4Addr::UNSPECIFIED);
    m.options
        .push((opt::REQUESTED_IP, requested.octets().to_vec()));
    m.options.push((opt::SERVER_ID, server.octets().to_vec()));
    push_common_tail(&mut m);
    m
}

/// INIT-REBOOT REQUEST: broadcast, requested address (50), no server id.
pub fn request_init_reboot(id: &Identity, xid: u32, secs: u16, requested: Ipv4Addr) -> Message {
    let mut m = base(id, xid, secs, MessageType::Request, Ipv4Addr::UNSPECIFIED);
    m.options
        .push((opt::REQUESTED_IP, requested.octets().to_vec()));
    push_common_tail(&mut m);
    m
}

/// RENEW / REBIND REQUEST: ciaddr = lease, no option 50 or 54. The caller
/// decides unicast (RENEW) or broadcast (REBIND) at the frame level.
pub fn request_renew(id: &Identity, xid: u32, secs: u16, ciaddr: Ipv4Addr) -> Message {
    let mut m = base(id, xid, secs, MessageType::Request, ciaddr);
    push_common_tail(&mut m);
    m
}

/// RELEASE: ciaddr = lease, server id (54). No reply is expected.
pub fn release(id: &Identity, xid: u32, ciaddr: Ipv4Addr, server: Ipv4Addr) -> Message {
    let mut m = base(id, xid, 0, MessageType::Release, ciaddr);
    m.options.push((opt::SERVER_ID, server.octets().to_vec()));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

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
            (self.next() >> 56) as u8
        }
    }

    #[test]
    fn parsers_never_panic_on_random_input() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let id = Identity::new([2, 0, 0, 0, 0, 1], "fuzz");
        let template = discover(&id, 1, 0).encode();
        for round in 0..20_000 {
            let len = (rng.next() % 600) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            if round % 2 == 1 {
                // Half the rounds: start from a valid message and corrupt it,
                // so the option walker actually gets exercised past the cookie.
                buf = template.clone();
                let flips = 1 + (rng.next() % 12) as usize;
                for _ in 0..flips {
                    let i = (rng.next() % buf.len() as u64) as usize;
                    buf[i] = rng.byte();
                }
                let cut = (rng.next() % (buf.len() as u64 + 1)) as usize;
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
        let id = Identity::new([0xde, 0xad, 0xbe, 0xef, 0, 1], "routedroid:x:y");
        let m = request_selecting(
            &id,
            0xdead_beef,
            7,
            Ipv4Addr::new(10, 1, 2, 3),
            Ipv4Addr::new(10, 1, 2, 1),
        );
        let enc = m.encode();
        assert_eq!(enc.len(), 300);
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
            8, 10, 10, 17, 0, 1, 16, 10, 229, 10, 229, 0, 1, 0, 10, 27, 129, 1, 24, 10, 27, 129,
            10, 27, 129, 1,
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
        let id = Identity::new([2, 0, 0, 0, 0, 1], "routedroid:a:b");
        let lease = Ipv4Addr::new(192, 168, 50, 100);
        let server = Ipv4Addr::new(192, 168, 50, 1);
        let d = discover(&id, 1, 0);
        assert_eq!(d.flags, FLAG_BROADCAST);
        assert!(d.ciaddr.is_unspecified());
        assert_eq!(d.option(opt::CLIENT_ID).unwrap(), b"\0routedroid:a:b");
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
    }
}
