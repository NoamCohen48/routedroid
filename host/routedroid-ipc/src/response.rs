//! What the daemon answers: one response type per request, tagged on `"type"`.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::{Check, ConnectionState, Kind, Outcome, Phone, Traffic};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Version {
        daemon: String,
        api: u32,
    },
    Devices {
        devices: Vec<DeviceInfo>,
    },
    Interfaces {
        interfaces: Vec<InterfaceInfo>,
    },
    /// The connection is running, on the phone, LAN and TUN it was given
    /// (any of which the daemon may have picked).
    Started {
        serial: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        lan_if: String,
        /// `None`: leasing one by DHCP.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phone_ip: Option<Ipv4Addr>,
        tun: String,
    },
    /// The connection has ended, and how.
    Stopped {
        serial: String,
        outcome: Outcome,
    },
    Status {
        connections: Vec<ConnectionInfo>,
    },
    Subscribed,
    Phones {
        phones: Vec<Phone>,
    },
    Remembered {
        phone: Phone,
    },
    Forgotten {
        phone: Phone,
    },
    Doctor {
        checks: Vec<Check>,
        /// The changes a repair made, in order.
        done: Vec<String>,
    },
    Error {
        kind: Kind,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub serial: String,
    /// Its remembered name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Remembered to connect when plugged in.
    #[serde(default)]
    pub auto: bool,
    /// adb's word: `device`, `unauthorized`, `offline`, ...
    pub state: String,
    pub model: Option<String>,
    /// `None` when this device can be connected; otherwise why not.
    pub unusable_reason: Option<String>,
    /// State of this device's connection, if it has one.
    pub connection: Option<ConnectionState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ipv4Net {
    pub address: Ipv4Addr,
    pub prefix: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub up: bool,
    pub addresses: Vec<Ipv4Net>,
    /// The host's default route leaves through this interface.
    pub default_route: bool,
    /// The blocks the helper's policy lets phones take here; empty if none.
    pub phone_addresses: Vec<Ipv4Net>,
    /// The policy lets phones lease an address here by DHCP.
    #[serde(default)]
    pub dhcp: bool,
    /// `None` when a phone may join the LAN through it; otherwise why not.
    pub ineligible: Option<String>,
}

/// One device connection: what it was asked for, and how it is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub serial: String,
    /// Its remembered name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub lan_if: String,
    pub tun: String,
    pub mtu: u32,
    pub state: ConnectionState,
    /// Unix seconds when `start` was accepted.
    pub started_at: u64,
    /// The phone's place on the LAN, once the helper has made it.
    pub network: Option<NetworkInfo>,
    pub traffic: Traffic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub phone_ip: Ipv4Addr,
    /// The host's own address on the LAN, and the LAN's prefix length.
    pub host_ip: Ipv4Addr,
    pub lan_prefix: u8,
    /// What the phone was told to resolve names with.
    pub dns: Vec<Ipv4Addr>,
    /// `None` for a static address.
    pub lease: Option<Lease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub server: Ipv4Addr,
    /// Unix seconds when the lease runs out unless renewed.
    pub expires_at: u64,
}
