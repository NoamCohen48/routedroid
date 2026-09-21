//! IPv4 header sanity check performed on both ends before injection
//! (protocol/phase0-draft.md, "IPv4 validation on both ends").

use std::fmt;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ipv4Error {
    /// Fewer than 20 bytes: no full IPv4 header.
    TooShort(usize),
    /// Version nibble is not 4.
    Version(u8),
    /// IHL < 5.
    Ihl(u8),
    /// total_length != body length.
    LengthMismatch { total_length: u16, body_length: usize },
    /// total_length < IHL*4.
    HeaderExceedsTotal { total_length: u16, header_length: usize },
}

impl fmt::Display for Ipv4Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "packet of {n} bytes shorter than IPv4 header"),
            Self::Version(v) => write!(f, "IP version {v} is not 4"),
            Self::Ihl(i) => write!(f, "IHL {i} < 5"),
            Self::LengthMismatch { total_length, body_length } => {
                write!(f, "IPv4 total_length {total_length} != frame body {body_length}")
            }
            Self::HeaderExceedsTotal { total_length, header_length } => {
                write!(f, "IPv4 total_length {total_length} < header length {header_length}")
            }
        }
    }
}

impl std::error::Error for Ipv4Error {}

/// Validate that `pkt` is exactly one IPv4 packet:
/// version == 4, IHL >= 5, total_length == pkt.len(), total_length >= IHL*4.
pub fn validate(pkt: &[u8]) -> Result<(), Ipv4Error> {
    if pkt.len() < 20 {
        return Err(Ipv4Error::TooShort(pkt.len()));
    }
    let version = pkt[0] >> 4;
    if version != 4 {
        return Err(Ipv4Error::Version(version));
    }
    let ihl = pkt[0] & 0x0f;
    if ihl < 5 {
        return Err(Ipv4Error::Ihl(ihl));
    }
    let total_length = u16::from_be_bytes([pkt[2], pkt[3]]);
    if usize::from(total_length) != pkt.len() {
        return Err(Ipv4Error::LengthMismatch { total_length, body_length: pkt.len() });
    }
    let header_length = usize::from(ihl) * 4;
    if usize::from(total_length) < header_length {
        return Err(Ipv4Error::HeaderExceedsTotal { total_length, header_length });
    }
    Ok(())
}

pub fn source(pkt: &[u8]) -> Ipv4Addr {
    Ipv4Addr::new(pkt[12], pkt[13], pkt[14], pkt[15])
}

pub fn destination(pkt: &[u8]) -> Ipv4Addr {
    Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(total: u16, len: usize) -> Vec<u8> {
        let mut p = vec![0u8; len];
        p[0] = 0x45;
        p[2..4].copy_from_slice(&total.to_be_bytes());
        p
    }

    #[test]
    fn accepts_minimal_packet() {
        assert_eq!(validate(&packet(28, 28)), Ok(()));
        let mut opts = packet(40, 40);
        opts[0] = 0x46; // IHL 6
        assert_eq!(validate(&opts), Ok(()));
    }

    #[test]
    fn rejects_short_version_ihl_length() {
        assert_eq!(validate(&[0x45; 19]), Err(Ipv4Error::TooShort(19)));
        let mut v6 = packet(28, 28);
        v6[0] = 0x65;
        assert_eq!(validate(&v6), Err(Ipv4Error::Version(6)));
        let mut ihl = packet(28, 28);
        ihl[0] = 0x44;
        assert_eq!(validate(&ihl), Err(Ipv4Error::Ihl(4)));
        assert_eq!(validate(&packet(30, 28)), Err(Ipv4Error::LengthMismatch { total_length: 30, body_length: 28 }));
        assert_eq!(validate(&packet(27, 28)), Err(Ipv4Error::LengthMismatch { total_length: 27, body_length: 28 }));
        let mut big_ihl = packet(28, 28);
        big_ihl[0] = 0x4f; // IHL 15 -> 60 byte header > total 28
        assert_eq!(validate(&big_ihl), Err(Ipv4Error::HeaderExceedsTotal { total_length: 28, header_length: 60 }));
    }
}
