//! Folding daemon events into the app state.

use routedroid_ipc::{ConnectionState, Event};

use super::{App, Level};
use crate::describe;
use crate::messages::Command;

impl App {
    pub(super) fn apply_event(&mut self, event: Event) -> Vec<Command> {
        if let Some(line) = describe::event(&event) {
            let level = match &event {
                Event::Connection { state, .. } if describe::is_failure(state) => Level::Error,
                Event::Shutdown => Level::Error,
                _ => Level::Info,
            };
            self.push_log(level, line);
        }
        match event {
            Event::Connection { serial, state } => self.apply_connection(serial, state),
            Event::Traffic { serial, packets_to_phone, packets_from_phone } => {
                if let Some(connection) = self.connections.get_mut(&serial) {
                    connection.packets_to_phone = packets_to_phone;
                    connection.packets_from_phone = packets_from_phone;
                }
                vec![]
            }
            Event::Devices { devices } => {
                self.set_devices(devices);
                vec![]
            }
            Event::Shutdown => vec![],
            Event::Lagged { .. } => vec![Command::RefreshDevices, Command::RefreshStatus],
        }
    }

    fn apply_connection(&mut self, serial: String, state: ConnectionState) -> Vec<Command> {
        let ended = matches!(state, ConnectionState::Ended(_));
        if let Some(device) = self.devices.iter_mut().find(|device| device.serial == serial) {
            device.connection = if ended { None } else { Some(state.clone()) };
        }
        if ended {
            self.connections.remove(&serial);
            return vec![];
        }
        match self.connections.get_mut(&serial) {
            Some(connection) => {
                connection.state = state;
                vec![]
            }
            // A connection we have no details for yet (just started): fetch them.
            None => vec![Command::RefreshStatus],
        }
    }
}
