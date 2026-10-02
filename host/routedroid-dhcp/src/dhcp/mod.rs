//! BOOTP/DHCP message codec (RFC 2131/2132/3396/3442) and the message
//! builders for every state in architecture.md §6.1. Pure functions.

use std::fmt;

mod build;
mod error;
mod message;
mod options;

pub use build::{
    Identity, decline, discover, release, request_init_reboot, request_renew, request_selecting,
};
pub use error::ParseError;
pub use message::{Message, parse_options};
pub use options::{Options, StaticRoute, parse_classless_routes};

pub const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
pub const FLAG_BROADCAST: u16 = 0x8000;
pub const CLIENT_PORT: u16 = 68;
pub const SERVER_PORT: u16 = 67;
pub const BOOTREQUEST: u8 = 1;
pub const BOOTREPLY: u8 = 2;
pub const HTYPE_ETHERNET: u8 = 1;
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

/// Option 53 values 1..=8, in order, with their names.
const TYPES: [(MessageType, &str); 8] = [
    (MessageType::Discover, "DISCOVER"),
    (MessageType::Offer, "OFFER"),
    (MessageType::Request, "REQUEST"),
    (MessageType::Decline, "DECLINE"),
    (MessageType::Ack, "ACK"),
    (MessageType::Nak, "NAK"),
    (MessageType::Release, "RELEASE"),
    (MessageType::Inform, "INFORM"),
];

impl MessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        let i = usize::from(v).checked_sub(1)?;
        TYPES.get(i).map(|(t, _)| *t)
    }

    pub fn as_u8(self) -> u8 {
        self as u8 + 1
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(TYPES[*self as usize].1)
    }
}

#[cfg(test)]
mod tests;
