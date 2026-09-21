//! Control messages between the unprivileged controller and the helper.
//! Transport: one Unix SOCK_SEQPACKET connection; every datagram is
//! `[kind u8][payload]`, kind 0x01 = JSON control, 0x10 = raw IPv4 packet.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

pub const KIND_CONTROL: u8 = 0x01;
pub const KIND_PACKET: u8 = 0x10;
/// Largest datagram either side sends (packet + kind byte).
pub const MAX_DATAGRAM: usize = 65536 + 1;
/// Receive-buffer size: one byte more than any legal datagram, so a datagram
/// that fills the buffer is known to have been truncated (`SeqPacket::recv`).
pub const RECV_BUF: usize = MAX_DATAGRAM + 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Bring up one phone session. The helper derives the host address and
    /// LAN prefix from `lan_if` itself; the client cannot pick them.
    Start {
        lan_if: String,
        phone_ip: Ipv4Addr,
        tun: String,
        mtu: u32,
    },
    /// Undo everything, reply `Stopped`, then exit.
    Stop,
    Ping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
    Started { session: String, tun: String, host_ip: Ipv4Addr, lan_prefix: u8 },
    Stopped,
    Pong,
    Error { code: String, message: String },
}

/// Interface names: 1..=15 chars of [A-Za-z0-9_.-], never "lo".
pub fn valid_ifname(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 16
        && s != "lo"
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ifnames() {
        assert!(valid_ifname("eno1"));
        assert!(valid_ifname("phone0"));
        assert!(!valid_ifname("lo"));
        assert!(!valid_ifname(""));
        assert!(!valid_ifname("a b"));
        assert!(!valid_ifname("x/../y"));
        assert!(!valid_ifname("0123456789abcdef"));
    }

    #[test]
    fn json_shape() {
        let r = Request::Start {
            lan_if: "eno1".into(),
            phone_ip: "10.0.0.5".parse().unwrap(),
            tun: "phone0".into(),
            mtu: 1400,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"type":"start","lan_if":"eno1","phone_ip":"10.0.0.5","tun":"phone0","mtu":1400}"#
        );
        let s: Request = serde_json::from_str(r#"{"type":"stop"}"#).unwrap();
        assert_eq!(s, Request::Stop);
    }
}
