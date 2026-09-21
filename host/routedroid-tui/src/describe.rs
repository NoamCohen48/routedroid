//! Human-readable one-liners for states and events, shared by the panes and the log.

use routedroid_ipc::{DeviceInfo, Event, SessionState};

pub fn session_state(state: &SessionState) -> String {
    match state {
        SessionState::Starting => "starting".into(),
        SessionState::WaitingForApp => "waiting for app".into(),
        SessionState::Handshaking => "handshaking".into(),
        SessionState::Active => "active".into(),
        SessionState::Stopping => "stopping".into(),
        SessionState::Ended(outcome) if outcome.ok => format!("ended: {}", outcome.message),
        SessionState::Ended(outcome) => match outcome.kind {
            Some(kind) => format!("failed ({}): {}", kind.as_str(), outcome.message),
            None => format!("failed: {}", outcome.message),
        },
    }
}

/// Whether an `Ended` state is a failure worth painting red.
pub fn is_failure(state: &SessionState) -> bool {
    matches!(state, SessionState::Ended(outcome) if !outcome.ok)
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
        Event::Session { serial, state } => Some(format!("{serial}: {}", session_state(state))),
        Event::Traffic { .. } => None,
        Event::Devices { devices } => {
            let serials: Vec<&str> = devices.iter().map(|device| device.serial.as_str()).collect();
            Some(format!("devices: {} attached [{}]", devices.len(), serials.join(", ")))
        }
        Event::Shutdown => Some("daemon is shutting down".into()),
    }
}
