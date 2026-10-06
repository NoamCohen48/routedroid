//! What a client may ask. Tagged on `"type"` so a client in any language
//! can build one by hand.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

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
    /// Disconnect one device; answered `stopped` once its connection has ended.
    Stop { serial: String },
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
    pub serial: String,
    pub lan_if: String,
    /// The phone's LAN address; `None` leases one from the LAN's DHCP server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_ip: Option<Ipv4Addr>,
    /// TUN name; the daemon picks a free `phoneN` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    #[serde(default)]
    pub dns: DnsChoice,
    /// Seconds to wait for the app to connect after launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_secs: Option<u64>,
    /// Seconds an unplugged phone's address is held for it to come back;
    /// 0 ends the connection at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconnect_secs: Option<u64>,
    /// Connect over a network ADB serial despite decision 0001 gate 5.
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
