//! Folding daemon events into the app state.

use routedroid_ipc::{ConnectionState, Event};

use super::log::now;
use super::{App, LastEnd, Level};
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
            self.log.push(level, line);
        }
        match event {
            Event::Connection { serial, state } => self.apply_connection(serial, state),
            Event::Network { serial, network } => match self.connections.get_mut(&serial) {
                Some(connection) => {
                    connection.network = Some(network);
                    vec![]
                }
                None => vec![Command::RefreshStatus],
            },
            Event::Traffic { serial, traffic } => {
                if let Some(connection) = self.connections.get_mut(&serial) {
                    connection.traffic = traffic;
                }
                vec![]
            }
            Event::Devices { devices } => self.set_devices(devices),
            Event::Shutdown => vec![],
            Event::Lagged { .. } => vec![Command::RefreshDevices, Command::RefreshStatus],
        }
    }

    fn apply_connection(&mut self, serial: String, state: ConnectionState) -> Vec<Command> {
        if let Some(device) = self
            .devices
            .iter_mut()
            .find(|device| device.serial == serial)
        {
            device.connection = match state {
                ConnectionState::Ended { .. } => None,
                _ => Some(state.clone()),
            };
        }
        if let ConnectionState::Ended { outcome } = state {
            self.connections.remove(&serial);
            let end = LastEnd {
                text: outcome.to_string(),
                failed: !outcome.is_clean(),
                time: now(),
            };
            self.last_end.insert(serial, end);
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
