use std::fmt;

/// Why a frame is not one well-formed, unfragmented IPv4/UDP datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    TooShort(usize),
    NotIpv4(u16),
    Version(u8),
    Ihl(u8),
    Fragment,
    NotUdp(u8),
    IpLength { total_length: u16, available: usize },
    IpChecksum,
    UdpLength { udp_length: u16, available: usize },
    UdpChecksum,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "frame of {n} bytes too short"),
            Self::NotIpv4(t) => write!(f, "ethertype {t:#06x} is not IPv4"),
            Self::Version(v) => write!(f, "IP version {v} is not 4"),
            Self::Ihl(i) => write!(f, "IHL {i} < 5"),
            Self::Fragment => write!(f, "fragmented IPv4 packet"),
            Self::NotUdp(p) => write!(f, "IP protocol {p} is not UDP"),
            Self::IpLength {
                total_length,
                available,
            } => write!(
                f,
                "IPv4 total_length {total_length} does not fit the frame body {available}"
            ),
            Self::IpChecksum => write!(f, "bad IPv4 header checksum"),
            Self::UdpLength {
                udp_length,
                available,
            } => write!(
                f,
                "UDP length {udp_length} does not fit in {available} bytes"
            ),
            Self::UdpChecksum => write!(f, "bad UDP checksum"),
        }
    }
}

impl std::error::Error for PacketError {}
