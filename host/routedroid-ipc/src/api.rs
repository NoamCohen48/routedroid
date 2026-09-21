//! Requests, responses and events. Every enum is tagged so a client written
//! in another language can pattern-match on `"type"`.

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::fault::Kind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Daemon and API version.
    Version,
    /// Attached devices, with whether each can be started and any live session.
    Devices,
    /// Start a session on one device; answered as soon as it is accepted
    /// (progress arrives as `session` events).
    Start(StartRequest),
    /// Stop the session on one device; answered once the session has ended.
    Stop { serial: String },
    /// All live sessions.
    Status,
    /// Receive `Event`s on this connection from now on.
    Subscribe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRequest {
    pub serial: String,
    pub lan_if: String,
    pub phone_ip: Ipv4Addr,
    /// TUN name; the daemon picks a free `phoneN` when absent.
    #[serde(default)]
    pub tun: Option<String>,
    #[serde(default)]
    pub mtu: Option<u32>,
    #[serde(default)]
    pub dns: Vec<Ipv4Addr>,
    /// Seconds to wait for the app to connect after launch.
    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,
    /// Start over a network ADB serial despite decision 0001 gate 5.
    #[serde(default)]
    pub allow_network_adb: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Version { daemon: String, api: u32 },
    Devices { devices: Vec<DeviceInfo> },
    Started { serial: String },
    Status { sessions: Vec<SessionInfo> },
    Error { kind: Kind, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub serial: String,
    /// adb's word: `device`, `unauthorized`, `offline`, ...
    pub state: String,
    pub model: Option<String>,
    /// `None` when a session can be started; otherwise why not.
    pub unusable_reason: Option<String>,
    /// Live session on this device, if any.
    pub session: Option<SessionState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub serial: String,
    pub lan_if: String,
    pub phone_ip: Ipv4Addr,
    pub tun: String,
    pub state: SessionState,
    /// Unix seconds when `start` was accepted.
    pub started_at: u64,
    pub packets_to_phone: u64,
    pub packets_from_phone: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    /// Helper session and reverse mapping being set up.
    Starting,
    /// App launched; waiting for it to connect (consent dialog may be up).
    WaitingForApp,
    /// Connected; HELLO/AUTH/CONFIGURE in progress.
    Handshaking,
    Active,
    Stopping,
    Ended(Outcome),
}

/// How a session ended. `kind` follows the CLI exit-code taxonomy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// A session changed state (including its final `ended`).
    Session { serial: String, state: SessionState },
    /// Periodic counters for an active session.
    Traffic { serial: String, packets_to_phone: u64, packets_from_phone: u64 },
    /// A device appeared or went away, or changed adb state.
    Devices { devices: Vec<DeviceInfo> },
    /// The daemon is shutting down; sessions are being stopped.
    Shutdown,
}
