//! The BOOTP message and its TLV option block.

use std::net::Ipv4Addr;

use super::{MAGIC_COOKIE, MessageType, ParseError, opt};
use crate::packet::Mac;

/// Fixed BOOTP header up to and including `file`.
const FIXED_LEN: usize = 236;
/// Minimum BOOTP payload; some relays and servers drop shorter datagrams.
const MIN_PAYLOAD: usize = 300;

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
    /// happens in [`Message::option`]). Each body is at most 255 bytes.
    pub options: Vec<(u8, Vec<u8>)>,
}

/// Parse a TLV option block. PAD is skipped, END stops; anything else must
/// carry a complete length and body. Never panics.
pub fn parse_options(buf: &[u8], out: &mut Vec<(u8, Vec<u8>)>) -> Result<(), ParseError> {
    let mut i = 0;
    while let Some(&code) = buf.get(i) {
        match code {
            opt::PAD => i += 1,
            opt::END => return Ok(()),
            _ => {
                let Some(&len) = buf.get(i + 1) else {
                    return Err(ParseError::DanglingCode { code, at: i });
                };
                let end = i + 2 + usize::from(len);
                let Some(body) = buf.get(i + 2..end) else {
                    return Err(ParseError::TruncatedOption { code, at: i });
                };
                out.push((code, body.to_vec()));
                i = end;
            }
        }
    }
    Ok(())
}

fn ip_at(b: &[u8], at: usize) -> Ipv4Addr {
    Ipv4Addr::new(b[at], b[at + 1], b[at + 2], b[at + 3])
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

impl Message {
    /// Strict decode: fixed header, magic cookie, well-formed options, and
    /// option overload (52) into `file` and `sname`.
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
            secs: u16_at(buf, 8),
            flags: u16_at(buf, 10),
            ciaddr: ip_at(buf, 12),
            yiaddr: ip_at(buf, 16),
            siaddr: ip_at(buf, 20),
            giaddr: ip_at(buf, 24),
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
        for a in [self.ciaddr, self.yiaddr, self.siaddr, self.giaddr] {
            b.extend_from_slice(&a.octets());
        }
        b.extend_from_slice(&self.chaddr);
        b.resize(FIXED_LEN, 0); // sname + file
        b.extend_from_slice(&MAGIC_COOKIE);
        for (code, body) in &self.options {
            let len = u8::try_from(body.len()).expect("option bodies fit one length byte");
            b.push(*code);
            b.push(len);
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
        let mut parts = self.options.iter().filter(|(c, _)| *c == code).peekable();
        parts.peek()?;
        Some(parts.flat_map(|(_, body)| body.iter().copied()).collect())
    }

    pub fn message_type(&self) -> Option<MessageType> {
        match self.option(opt::MESSAGE_TYPE)?.as_slice() {
            [v] => MessageType::from_u8(*v),
            _ => None,
        }
    }

    pub fn chaddr_mac(&self) -> Mac {
        let mut m = [0u8; 6];
        m.copy_from_slice(&self.chaddr[..6]);
        m
    }
}
