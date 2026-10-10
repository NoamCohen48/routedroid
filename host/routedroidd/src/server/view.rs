//! Building the wire's view of a device: adb's row, our verdict about it, and
//! the state of its connection if it has one, and what is remembered of it. The join lives here because
//! neither component may depend on the other — and because a `DeviceInfo` is
//! something only the layer that speaks the protocol needs.

use std::collections::HashMap;

use routedroid_ipc::{ConnectionState, DeviceInfo, Phone};

use crate::daemon::Snapshot;
use crate::device::{state_name, unusable};

pub fn devices(
    attached: &Snapshot,
    connections: &HashMap<String, ConnectionState>,
    phones: &[Phone],
) -> Vec<DeviceInfo> {
    attached
        .iter()
        .map(|device| {
            let phone = phones.iter().find(|p| p.serial == device.serial);
            (device, phone)
        })
        .map(|(device, phone)| DeviceInfo {
            name: phone.and_then(|p| p.name.clone()),
            auto: phone.is_some_and(|p| p.auto),
            unusable_reason: unusable(&device.state, &device.serial).map(str::to_string),
            connection: connections.get(&device.serial).cloned(),
            state: state_name(&device.state),
            serial: device.serial.clone(),
            model: device.model.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::adb::{Device, DeviceState};

    use super::*;

    fn device(serial: &str, state: DeviceState) -> Device {
        Device {
            serial: serial.into(),
            state,
            model: Some("SM_J810G".into()),
        }
    }

    #[test]
    fn joins_adb_rows_with_connection_states() {
        let attached = Arc::new(vec![
            device("usb-one", DeviceState::Device),
            device("usb-two", DeviceState::Device),
            device("usb-three", DeviceState::Unauthorized),
        ]);
        let connections = HashMap::from([("usb-two".to_string(), ConnectionState::Active)]);
        let phones = [Phone {
            serial: "usb-two".into(),
            name: Some("pixel".into()),
            auto: true,
            ..Phone::default()
        }];
        let view = devices(&attached, &connections, &phones);
        assert_eq!(view[1].name.as_deref(), Some("pixel"));
        assert!(view[1].auto && !view[0].auto);
        assert_eq!(view[0].connection, None);
        assert_eq!(view[0].unusable_reason, None);
        assert_eq!(view[1].connection, Some(ConnectionState::Active));
        assert_eq!(view[2].state, "unauthorized");
        assert!(view[2].unusable_reason.is_some());
    }

    #[test]
    fn a_connection_without_an_attached_device_is_not_a_row() {
        let attached = Arc::new(vec![device("usb-one", DeviceState::Device)]);
        let connections = HashMap::from([("gone".to_string(), ConnectionState::Stopping)]);
        assert_eq!(devices(&attached, &connections, &[]).len(), 1);
    }
}
