//! The options a client acts on, decoded leniently: a malformed individual
//! option is reported in `malformed` and otherwise ignored.

use std::fmt;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use super::{Message, MessageType, opt};

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

fn ip4(b: &[u8]) -> Ipv4Addr {
    Ipv4Addr::new(b[0], b[1], b[2], b[3])
}

/// RFC 3442 classless static routes: the routes decoded before the first
/// malformed entry, and whether the whole option was well-formed.
pub fn parse_classless_routes(b: &[u8]) -> (Vec<StaticRoute>, bool) {
    let mut out = Vec::new();
    let mut rest = b;
    while let Some((&prefix, tail)) = rest.split_first() {
        if prefix > 32 {
            return (out, false);
        }
        let n = usize::from(prefix).div_ceil(8);
        let Some(entry) = tail.get(..n + 4) else {
            return (out, false);
        };
        let mut dest = [0u8; 4];
        dest[..n].copy_from_slice(&entry[..n]);
        let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
        out.push(StaticRoute {
            dest: Ipv4Addr::from(u32::from_be_bytes(dest) & mask), // canonical: host bits clear
            prefix,
            router: ip4(&entry[n..]),
        });
        rest = &tail[n + 4..];
    }
    (out, true)
}

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
    /// Option 61 as the server echoed it (RFC 6842), type byte included.
    pub client_id: Option<Vec<u8>>,
    pub malformed: Vec<u8>,
}

fn ip_list(b: &[u8]) -> Option<Vec<Ipv4Addr>> {
    let (chunks, rest) = b.as_chunks::<4>();
    (!chunks.is_empty() && rest.is_empty()).then(|| chunks.iter().map(|c| ip4(c)).collect())
}

fn one_ip(b: &[u8]) -> Option<Ipv4Addr> {
    (b.len() == 4).then(|| ip4(b))
}

fn u32_opt(b: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(b.try_into().ok()?))
}

/// `code`'s body parsed by `parse`; present but empty or undecodable is
/// recorded in `malformed`.
fn decode<T>(
    m: &Message,
    code: u8,
    malformed: &mut Vec<u8>,
    parse: impl FnOnce(&[u8]) -> Option<T>,
) -> Option<T> {
    let body = m.option(code)?;
    let parsed = (!body.is_empty()).then(|| parse(&body)).flatten();
    if parsed.is_none() {
        malformed.push(code);
    }
    parsed
}

fn message_type(b: &[u8]) -> Option<MessageType> {
    match b {
        [v] => MessageType::from_u8(*v),
        _ => None,
    }
}

impl Options {
    pub fn from_message(m: &Message) -> Self {
        let mut bad = Vec::new();
        let mut o = Self {
            message_type: decode(m, opt::MESSAGE_TYPE, &mut bad, message_type),
            subnet_mask: decode(m, opt::SUBNET_MASK, &mut bad, one_ip),
            routers: decode(m, opt::ROUTER, &mut bad, ip_list).unwrap_or_default(),
            dns: decode(m, opt::DNS, &mut bad, ip_list).unwrap_or_default(),
            server_id: decode(m, opt::SERVER_ID, &mut bad, one_ip),
            requested_ip: decode(m, opt::REQUESTED_IP, &mut bad, one_ip),
            lease_secs: decode(m, opt::LEASE_TIME, &mut bad, u32_opt),
            t1: decode(m, opt::T1, &mut bad, u32_opt),
            t2: decode(m, opt::T2, &mut bad, u32_opt),
            message: decode(m, opt::MESSAGE, &mut bad, |b| {
                Some(String::from_utf8_lossy(b).into_owned())
            }),
            client_id: decode(m, opt::CLIENT_ID, &mut bad, |b| Some(b.to_vec())),
            classless_routes: Vec::new(),
            malformed: Vec::new(),
        };
        // Routes before a malformed entry are kept, and the option still flagged.
        if let Some((routes, ok)) = decode(m, opt::CLASSLESS_ROUTES, &mut bad, |b| {
            Some(parse_classless_routes(b))
        }) {
            if !ok {
                bad.push(opt::CLASSLESS_ROUTES);
            }
            o.classless_routes = routes;
        }
        o.malformed = bad;
        o
    }
}
