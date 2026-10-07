//! The device table's rows: every phone adb lists, then every connection
//! whose phone it no longer lists (unplugged and reconnecting), so a live
//! connection never drops off the screen. The cursor stays on its phone as
//! rows come and go, and a device list is logged only when it changes.

use routedroid_ipc::DeviceInfo;

use super::App;
use crate::describe;
use crate::messages::Command;

impl App {
    pub(super) fn set_devices(&mut self, devices: Vec<DeviceInfo>) -> Vec<Command> {
        let line = describe::devices(&devices);
        if self.devices_line.as_ref() != Some(&line) {
            self.info(line.clone());
            self.devices_line = Some(line);
        }
        self.attached = devices;
        self.rebuild_rows();
        vec![]
    }

    pub(super) fn rebuild_rows(&mut self) {
        let selected = self.selected_device().map(|device| device.serial.clone());
        let mut rows = self.attached.clone();
        for row in &mut rows {
            if let Some(connection) = self.connections.get(&row.serial) {
                row.connection = Some(connection.state.clone());
            }
        }
        let away: Vec<DeviceInfo> = self
            .connections
            .values()
            .filter(|connection| !rows.iter().any(|row| row.serial == connection.serial))
            .map(|connection| DeviceInfo {
                serial: connection.serial.clone(),
                name: connection.name.clone(),
                auto: false,
                state: "gone".into(),
                model: None,
                // The ADB column says why ("gone"); the narrow usable one just says no.
                unusable_reason: Some(String::new()),
                connection: Some(connection.state.clone()),
            })
            .collect();
        rows.extend(away);
        self.devices = rows;
        if let Some(at) = selected.and_then(|s| self.devices.iter().position(|d| d.serial == s)) {
            self.cursor = at;
        }
        self.move_cursor(0);
    }
}
