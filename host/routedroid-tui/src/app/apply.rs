//! Folding the daemon's answers and events into the UI state.

use super::{App, DaemonLink, LastEnd, Mode, log};
use crate::messages::{Command, Incoming};

impl App {
    /// Folds one message in; returns what to ask the daemon next.
    pub fn apply(&mut self, incoming: Incoming) -> Vec<Command> {
        match incoming {
            Incoming::Connected => {
                self.daemon = DaemonLink::Connected;
                self.info("connected to routedroidd");
                vec![
                    Command::RefreshDevices,
                    Command::RefreshStatus,
                    Command::RefreshInterfaces,
                ]
            }
            Incoming::Disconnected { reason } => {
                self.error(format!("disconnected: {reason}"));
                self.daemon = DaemonLink::Disconnected { reason };
                vec![]
            }
            Incoming::Event(event) => self.apply_event(event),
            Incoming::Devices(devices) => self.set_devices(devices),
            Incoming::Connections(connections) => {
                let by_serial = connections.into_iter().map(|c| (c.serial.clone(), c));
                self.connections = by_serial.collect();
                self.rebuild_rows();
                vec![]
            }
            Incoming::Interfaces(interfaces) => {
                if let Mode::StartForm(form) = &mut self.mode {
                    form.offer(&interfaces);
                }
                self.interfaces = interfaces;
                vec![]
            }
            Incoming::Started {
                serial,
                lan_if,
                tun,
            } => {
                self.last_end.remove(&serial);
                self.rates.remove(&serial);
                self.info(format!("{serial}: start accepted on {lan_if} (TUN {tun})"));
                vec![]
            }
            Incoming::Remembered { label, auto } => {
                let when = if auto {
                    "whenever it is plugged in"
                } else {
                    "when asked"
                };
                self.info(format!("remembered {label}: it connects {when}"));
                vec![Command::RefreshDevices]
            }
            Incoming::Forgotten { label } => {
                self.info(format!("forgot {label}"));
                vec![Command::RefreshDevices]
            }
            Incoming::Stopped => vec![],
            Incoming::Failed {
                what,
                serial,
                message,
            } => {
                if let (Some(serial), "start") = (&serial, what) {
                    let text = format!("start refused: {message}");
                    let end = LastEnd {
                        text,
                        failed: true,
                        time: log::now(),
                    };
                    self.last_end.insert(serial.clone(), end);
                }
                let about = serial
                    .map(|serial| format!("{serial}: "))
                    .unwrap_or_default();
                self.error(format!("{about}{what} failed: {message}"));
                vec![]
            }
        }
    }
}
