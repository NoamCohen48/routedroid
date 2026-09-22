//! What flows between the UI and the task that owns the daemon connection.

use routedroid_ipc::{ConnectionInfo, DeviceInfo, Event, StartRequest};

/// UI → client task: something to ask the daemon.
#[derive(Debug, Clone)]
pub enum Command {
    RefreshDevices,
    RefreshStatus,
    Start(StartRequest),
    Stop { serial: String },
}

/// Client task → UI: what the daemon said, or what happened to the connection.
#[derive(Debug, Clone)]
pub enum Incoming {
    Connected,
    Disconnected {
        reason: String,
    },
    Event(Event),
    Devices(Vec<DeviceInfo>),
    Connections(Vec<ConnectionInfo>),
    Started {
        serial: String,
    },
    Stopped {
        serial: String,
    },
    /// A request failed; `what` names it for the log line.
    Failed {
        what: String,
        message: String,
    },
}
