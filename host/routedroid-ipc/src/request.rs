//! What a client may ask. Tagged on `"type"` so a client in any language
//! can build one by hand.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::Phone;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Daemon and API version.
    Version,
    /// Attached devices, with whether each can be connected and any live connection.
    Devices,
    /// The host's network interfaces, with whether a phone may join through each.
    Interfaces,
    /// Connect one device; answered `started` as soon as the request is
    /// accepted (progress arrives as `connection` events). Every field is
    /// checked before that answer: a refused start starts nothing.
    Start(StartRequest),
    /// Disconnect one device (by serial or name); answered `stopped` once
    /// its connection has ended.
    Stop { serial: String },
    /// The remembered phones.
    Phones,
    /// Remember a phone, replacing what was remembered for its serial;
    /// answered `remembered`.
    Remember(Phone),
    /// Forget a phone (by serial or name); answered `forgotten`.
    Forget { phone: String },
    /// Every live device connection.
    Status,
    /// Receive `Event`s on this connection from now on; answered `subscribed`.
    Subscribe,
    /// Check adb, the helper, its policy and what Routedroid left behind;
    /// with `repair`, also make the changes the checks name.
    Doctor {
        #[serde(default)]
        repair: bool,
    },
}

/// Every optional field has a daemon-owned default, so a client sends only
/// what its user chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    /// A serial or a remembered name; `None` picks the one phone attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// `None`: the phone's remembered one, else the one the policy allows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lan_if: Option<String>,
    /// The phone's LAN address; `None` leases one from the LAN's DHCP server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_ip: Option<Ipv4Addr>,
    /// TUN name; the daemon picks a free `phoneN` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    /// `None`: the phone's remembered choice, else `auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsChoice>,
    /// Seconds to wait for the app to connect after launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_secs: Option<u64>,
    /// Seconds an unplugged phone's address is held for it to come back;
    /// 0 ends the connection at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_secs: Option<u64>,
    /// Connect over a network ADB serial (untested: the phone's traffic
    /// going through the PC may cut ADB itself).
    #[serde(default)]
    pub allow_network_adb: bool,
}

/// Which DNS servers the phone is given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsChoice {
    /// The DHCP lease's servers, else the LAN's default gateway.
    #[default]
    Auto,
    /// No DNS: the phone resolves nothing through the VPN.
    None,
    Servers(Vec<Ipv4Addr>),
}
