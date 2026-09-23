//! A network interface name, validated once at the IPC boundary so the
//! helper never builds a path, a sysctl key or an nft identifier from an
//! unchecked string.

use std::fmt;

use serde::{Deserialize, Serialize};

/// 1..=15 bytes of `[A-Za-z0-9_.-]`, not starting with `-`, and none of the
/// names that are not real devices in `/proc/sys/net/ipv4/conf`: `.`, `..`,
/// `all`, `default`, and `lo` (never a LAN or a phone interface).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IfName(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a usable interface name")]
pub struct IfNameError(String);

/// The kernel's IFNAMSIZ less the terminating NUL.
const MAX_LEN: usize = 15;
const RESERVED: [&str; 5] = [".", "..", "all", "default", "lo"];

impl IfName {
    pub fn new(name: impl Into<String>) -> Result<Self, IfNameError> {
        let name = name.into();
        let charset_ok = name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
        if name.is_empty() || name.len() > MAX_LEN || !charset_ok || name.starts_with('-') || RESERVED.contains(&&*name)
        {
            return Err(IfNameError(name));
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for IfName {
    type Error = IfNameError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::new(name)
    }
}

impl std::str::FromStr for IfName {
    type Err = IfNameError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::new(name)
    }
}

impl From<IfName> for String {
    fn from(name: IfName) -> Self {
        name.0
    }
}

impl fmt::Display for IfName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests;
