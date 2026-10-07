//! One-liners for the log and the panes. Connection states describe
//! themselves (`Display` in the ipc crate), the same way the CLI prints them.

use routedroid_ipc::{ConnectionState, DeviceInfo, Event};

/// Whether an `Ended` state is a failure worth painting red.
pub fn is_failure(state: &ConnectionState) -> bool {
    matches!(state, ConnectionState::Ended { outcome } if !outcome.is_clean())
}

pub fn usable(device: &DeviceInfo) -> String {
    match &device.unusable_reason {
        None => "yes".into(),
        Some(reason) if reason.is_empty() => "no".into(),
        Some(reason) => format!("no: {reason}"),
    }
}

pub fn devices(devices: &[DeviceInfo]) -> String {
    let serials: Vec<&str> = devices
        .iter()
        .map(|device| device.serial.as_str())
        .collect();
    format!(
        "devices: {} attached [{}]",
        devices.len(),
        serials.join(", ")
    )
}

/// One log line per event; `None` for events too frequent to log (traffic).
pub fn event(event: &Event) -> Option<String> {
    match event {
        Event::Connection { serial, state } => Some(format!("{serial}: {state}")),
        Event::Network { serial, network } => {
            Some(format!("{serial}: on the LAN as {}", network.phone_ip))
        }
        Event::Traffic { .. } => None,
        // Logged by the device table, when the list changes.
        Event::Devices { .. } => None,
        Event::Shutdown => Some("daemon is shutting down".into()),
        Event::Lagged { missed } => Some(format!("missed {missed} events; refreshing")),
    }
}
