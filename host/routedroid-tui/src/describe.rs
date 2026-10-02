//! Human-readable one-liners for states and events, shared by the panes and the log.

use routedroid_ipc::{ConnectionState, DeviceInfo, Event};

pub fn connection_state(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Starting => "starting".into(),
        ConnectionState::WaitingForApp => "waiting for app".into(),
        ConnectionState::Handshaking => "handshaking".into(),
        ConnectionState::Active => "active".into(),
        ConnectionState::Stopping => "stopping".into(),
        ConnectionState::Ended(outcome) if outcome.ok => format!("ended: {}", outcome.message),
        ConnectionState::Ended(outcome) => match outcome.kind {
            Some(kind) => format!("failed ({}): {}", kind.as_str(), outcome.message),
            None => format!("failed: {}", outcome.message),
        },
    }
}

/// Whether an `Ended` state is a failure worth painting red.
pub fn is_failure(state: &ConnectionState) -> bool {
    matches!(state, ConnectionState::Ended(outcome) if !outcome.ok)
}

pub fn usable(device: &DeviceInfo) -> String {
    match &device.unusable_reason {
        None => "yes".into(),
        Some(reason) => format!("no: {reason}"),
    }
}

/// One log line per event; `None` for events too frequent to log (traffic).
pub fn event(event: &Event) -> Option<String> {
    match event {
        Event::Connection { serial, state } => {
            Some(format!("{serial}: {}", connection_state(state)))
        }
        Event::Traffic { .. } => None,
        Event::Devices { devices } => {
            let serials: Vec<&str> = devices
                .iter()
                .map(|device| device.serial.as_str())
                .collect();
            Some(format!(
                "devices: {} attached [{}]",
                devices.len(),
                serials.join(", ")
            ))
        }
        Event::Shutdown => Some("daemon is shutting down".into()),
        Event::Lagged { missed } => Some(format!("missed {missed} events; refreshing")),
    }
}
