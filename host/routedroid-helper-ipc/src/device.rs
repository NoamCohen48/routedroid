//! A phone's identity as the helper and the LAN see it: 64 bits of SHA-256
//! over a domain tag and the ADB serial. The serial itself never leaves
//! the controller (architecture §6.1); the helper derives the DHCP client
//! identifier from this.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DOMAIN: &[u8] = b"routedroid device id v1\0";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceId([u8; 8]);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("device id must be 16 lowercase hex digits")]
pub struct BadDeviceId;

impl DeviceId {
    pub fn from_serial(serial: &str) -> Self {
        let digest = Sha256::new()
            .chain_update(DOMAIN)
            .chain_update(serial.as_bytes())
            .finalize();
        let mut id = [0u8; 8];
        id.copy_from_slice(&digest[..8]);
        Self(id)
    }

    pub fn bytes(&self) -> &[u8; 8] {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({self})")
    }
}

impl FromStr for DeviceId {
    type Err = BadDeviceId;

    fn from_str(s: &str) -> Result<Self, BadDeviceId> {
        let lower_hex = |b: &u8| matches!(b, b'0'..=b'9' | b'a'..=b'f');
        if s.len() != 16 || !s.as_bytes().iter().all(lower_hex) {
            return Err(BadDeviceId);
        }
        let mut id = [0u8; 8];
        for (byte, pair) in id.iter_mut().zip(s.as_bytes().chunks(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| BadDeviceId)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| BadDeviceId)?;
        }
        Ok(Self(id))
    }
}

impl TryFrom<String> for DeviceId {
    type Error = BadDeviceId;

    fn try_from(s: String) -> Result<Self, BadDeviceId> {
        s.parse()
    }
}

impl From<DeviceId> for String {
    fn from(id: DeviceId) -> Self {
        id.to_string()
    }
}

#[cfg(test)]
mod tests;
