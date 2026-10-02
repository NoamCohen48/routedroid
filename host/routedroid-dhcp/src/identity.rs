//! The option 61 client identifier (architecture §6.1):
//!
//! ```text
//! routedroid:<device>:<interface>
//! ```
//!
//! `<device>` is 16 hex digits of a one-way hash of the phone's ADB serial,
//! which the caller computes, so the serial never reaches the LAN.
//! `<interface>` is the interface MAC, so one phone gets one stable identity
//! per LAN. A free-form string is never sent: an identity is built from
//! those two parts, or parsed back from that exact shape.

use std::fmt;

use crate::packet::Mac;

const PREFIX: &str = "routedroid:";

/// Option 61 type 0 ("not a hardware address"), then the text above.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a Routedroid client id (routedroid:<16 hex>:<12 hex>)")]
pub struct BadClientId(String);

impl ClientId {
    pub fn new(device: &[u8; 8], mac: &Mac) -> Self {
        Self(format!("{PREFIX}{}:{}", hex(device), hex(mac)))
    }

    /// The `Display` form back, as a lease record stores it.
    pub fn parse(s: &str) -> Result<Self, BadClientId> {
        let well_formed = s
            .strip_prefix(PREFIX)
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(device, mac)| is_hex(device, 16) && is_hex(mac, 12));
        if well_formed {
            Ok(Self(s.to_owned()))
        } else {
            Err(BadClientId(s.to_owned()))
        }
    }

    /// The option 61 body.
    pub fn option(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.0.len() + 1);
        v.push(0);
        v.extend_from_slice(self.0.as_bytes());
        v
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests;
