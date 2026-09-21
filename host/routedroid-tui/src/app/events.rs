//! Folding daemon events into the app state.

use routedroid_ipc::{Event, SessionState};

use super::{App, Level};
use crate::describe;
use crate::messages::Command;

impl App {
    pub(super) fn apply_event(&mut self, event: Event) -> Vec<Command> {
        if let Some(line) = describe::event(&event) {
            let level = match &event {
                Event::Session { state, .. } if describe::is_failure(state) => Level::Error,
                Event::Shutdown => Level::Error,
                _ => Level::Info,
            };
            self.push_log(level, line);
        }
        match event {
            Event::Session { serial, state } => self.apply_session(serial, state),
            Event::Traffic { serial, packets_to_phone, packets_from_phone } => {
                if let Some(session) = self.sessions.get_mut(&serial) {
                    session.packets_to_phone = packets_to_phone;
                    session.packets_from_phone = packets_from_phone;
                }
                vec![]
            }
            Event::Devices { devices } => {
                self.set_devices(devices);
                vec![]
            }
            Event::Shutdown => vec![],
        }
    }

    fn apply_session(&mut self, serial: String, state: SessionState) -> Vec<Command> {
        let ended = matches!(state, SessionState::Ended(_));
        if let Some(device) = self.devices.iter_mut().find(|device| device.serial == serial) {
            device.session = if ended { None } else { Some(state.clone()) };
        }
        if ended {
            self.sessions.remove(&serial);
            return vec![];
        }
        match self.sessions.get_mut(&serial) {
            Some(session) => {
                session.state = state;
                vec![]
            }
            // A session we have no details for yet (just started): fetch them.
            None => vec![Command::RefreshStatus],
        }
    }
}
