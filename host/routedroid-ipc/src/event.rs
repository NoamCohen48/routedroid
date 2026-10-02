//! What the daemon pushes to a subscribed connection, tagged on `"event"`.

use serde::{Deserialize, Serialize};

use crate::{ConnectionState, DeviceInfo, NetworkInfo, Traffic};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// A device connection changed state (including its final `ended`).
    Connection {
        serial: String,
        state: ConnectionState,
    },
    /// The phone is on the LAN, or its lease was renewed.
    Network {
        serial: String,
        network: NetworkInfo,
    },
    /// An active connection's counters changed (at most once a second).
    Traffic { serial: String, traffic: Traffic },
    /// A device appeared or went away, or changed adb state.
    Devices { devices: Vec<DeviceInfo> },
    /// The daemon is shutting down; every connection is being stopped.
    Shutdown,
    /// This connection fell behind and `missed` events were dropped; the
    /// client should re-query `status` (and `devices`).
    Lagged { missed: u64 },
}
