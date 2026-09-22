//! Whether a device can be connected. adb says what state a phone is in; this
//! is our verdict about it, which is why it is not part of the device list.

use crate::adb::DeviceState;

use super::Transport;

/// `None` when this device can be connected; otherwise the reason it cannot.
pub fn unusable(state: &DeviceState, serial: &str) -> Option<&'static str> {
    match state {
        DeviceState::Unauthorized => Some("USB debugging not authorized on the phone"),
        DeviceState::Offline => Some("device is offline"),
        DeviceState::Other(_) => Some("device is not ready"),
        DeviceState::Device => Transport::classify(serial).refusal(false),
    }
}

/// adb's word for a state, as the wire reports it.
pub fn state_name(state: &DeviceState) -> String {
    match state {
        DeviceState::Other(other) => other.clone(),
        state => format!("{state:?}").to_lowercase(),
    }
}
