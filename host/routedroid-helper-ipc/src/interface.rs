//! The helper's answer to `Interfaces`: every link, and whether a phone may
//! join the LAN through it. The helper decides, because only it reads the
//! operator's policy.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// An IPv4 address or block with its prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    pub address: Ipv4Addr,
    pub prefix: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub name: String,
    pub up: bool,
    pub addresses: Vec<Net>,
    /// The host's default route leaves through this interface.
    pub default_route: bool,
    /// The blocks the policy lets phones take here; empty if none.
    pub phone_addresses: Vec<Net>,
    /// `None` when a phone may join the LAN through it; otherwise why not.
    pub ineligible: Option<String>,
}
