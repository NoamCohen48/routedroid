//! Message type codes and wire names (§3).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageType {
    Hello = 0x01,
    HelloAck = 0x02,
    ConfigureVpn = 0x03,
    VpnReady = 0x04,
    VpnError = 0x05,
    Auth = 0x06,
    IpPacket = 0x10,
    Ping = 0x20,
    Pong = 0x21,
    Stop = 0x30,
    Error = 0x7F,
}

impl MessageType {
    pub const ALL: [MessageType; 11] = [
        Self::Hello,
        Self::HelloAck,
        Self::ConfigureVpn,
        Self::VpnReady,
        Self::VpnError,
        Self::Auth,
        Self::IpPacket,
        Self::Ping,
        Self::Pong,
        Self::Stop,
        Self::Error,
    ];

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|t| *t as u8 == v)
    }

    /// Wire name as used in the spec and the fixtures.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hello => "HELLO",
            Self::HelloAck => "HELLO_ACK",
            Self::ConfigureVpn => "CONFIGURE_VPN",
            Self::VpnReady => "VPN_READY",
            Self::VpnError => "VPN_ERROR",
            Self::Auth => "AUTH",
            Self::IpPacket => "IP_PACKET",
            Self::Ping => "PING",
            Self::Pong => "PONG",
            Self::Stop => "STOP",
            Self::Error => "ERROR",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Messages whose body must be empty.
    pub fn is_empty_body(self) -> bool {
        matches!(self, Self::Ping | Self::Pong | Self::Stop)
    }

    pub fn is_control(self) -> bool {
        self != Self::IpPacket
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
