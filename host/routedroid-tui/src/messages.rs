//! What flows between the UI and the task that owns the daemon connection.

use routedroid_ipc::{ConnectionInfo, DeviceInfo, Event, InterfaceInfo, Phone, StartRequest};

/// UI → client task: something to ask the daemon.
#[derive(Debug, Clone)]
pub enum Command {
    RefreshDevices,
    RefreshStatus,
    RefreshInterfaces,
    /// Start, then remember the phone (on the LAN it was given) if `remember`.
    Start {
        request: StartRequest,
        remember: Option<Box<Phone>>,
    },
    Stop {
        serial: String,
    },
    Forget {
        phone: String,
    },
}

/// Client task → UI: what the daemon said, or what happened to the connection.
#[derive(Debug, Clone)]
pub enum Incoming {
    /// Connected and subscribed; the UI asks for whatever it needs next.
    Connected,
    Disconnected {
        reason: String,
    },
    Event(Event),
    Devices(Vec<DeviceInfo>),
    Connections(Vec<ConnectionInfo>),
    Interfaces(Vec<InterfaceInfo>),
    Started {
        serial: String,
        lan_if: String,
        tun: String,
    },
    /// The phone is remembered (or forgotten), as `label`.
    Remembered {
        label: String,
        auto: bool,
    },
    Forgotten {
        label: String,
    },
    /// A stop was answered; the connection's `ended` event says how it ended.
    Stopped,
    /// A request failed; `what` names it for the log line, and `serial`
    /// the phone it was about, if any.
    Failed {
        what: &'static str,
        serial: Option<String>,
        message: String,
    },
}
