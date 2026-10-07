//! A remembered phone: a name to call it by, the options it connects with,
//! and whether plugging it in connects it.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::DnsChoice;

/// What `start` falls back on for a phone it knows, field by field: an
/// option given to `start` wins over the remembered one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phone {
    pub serial: String,
    /// Usable wherever a serial is: `routedroid start pixel`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Connect when it is plugged in (and when the daemon starts with it attached).
    #[serde(default)]
    pub auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lan_if: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_ip: Option<Ipv4Addr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_secs: Option<u64>,
}

impl Phone {
    /// "galaxy (85e49002)", or the serial alone.
    pub fn label(&self) -> String {
        label(&self.serial, self.name.as_deref())
    }
}

/// "galaxy (85e49002)" for a named phone, else the serial.
pub fn label(serial: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("{name} ({serial})"),
        None => serial.to_string(),
    }
}
