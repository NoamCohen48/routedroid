//! UI state: what the panes draw. Pure `apply` methods fold daemon messages in;
//! they return follow-up commands instead of talking to the daemon themselves.

use std::collections::{BTreeMap, HashMap};

use routedroid_ipc::{ConnectionInfo, DeviceInfo, InterfaceInfo};

use crate::form::StartForm;
use crate::messages::{Command, Incoming};

mod events;
mod log;

pub use log::{Level, Log};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    StartForm(Box<StartForm>),
    ConfirmStop { serial: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonLink {
    Connecting,
    Connected,
    Disconnected { reason: String },
}

/// How a phone's last connection (or start attempt) ended, kept on its row
/// until it is started again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastEnd {
    pub text: String,
    pub failed: bool,
    pub time: String,
}

pub struct App {
    pub devices: Vec<DeviceInfo>,
    pub connections: BTreeMap<String, ConnectionInfo>,
    pub interfaces: Vec<InterfaceInfo>,
    pub last_end: HashMap<String, LastEnd>,
    /// The form as each phone's user last left it.
    drafts: HashMap<String, StartForm>,
    pub cursor: usize,
    pub log: Log,
    pub daemon: DaemonLink,
    pub mode: Mode,
    pub quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            connections: BTreeMap::new(),
            interfaces: Vec::new(),
            last_end: HashMap::new(),
            drafts: HashMap::new(),
            cursor: 0,
            log: Log::default(),
            daemon: DaemonLink::Connecting,
            mode: Mode::Normal,
            quit: false,
        }
    }

    pub fn selected_device(&self) -> Option<&DeviceInfo> {
        self.devices.get(self.cursor)
    }

    pub fn selected_connection(&self) -> Option<&ConnectionInfo> {
        self.connections.get(&self.selected_device()?.serial)
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.devices.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.log.push(Level::Info, text.into());
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.log.push(Level::Error, text.into());
    }

    /// The form for `serial`: its draft if it has one, else a fresh one.
    pub fn open_form(&mut self, serial: String) -> Vec<Command> {
        let form = match self.drafts.get(&serial) {
            Some(draft) => {
                let mut form = draft.clone();
                form.offer(&self.interfaces);
                form
            }
            None => StartForm::new(serial, &self.interfaces),
        };
        self.mode = Mode::StartForm(Box::new(form));
        vec![Command::RefreshInterfaces]
    }

    /// Leave the form, keeping what was typed for next time.
    pub fn close_form(&mut self, form: Box<StartForm>) {
        self.drafts.insert(form.serial.clone(), *form);
        self.mode = Mode::Normal;
    }

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
                vec![]
            }
            Incoming::Interfaces(interfaces) => {
                if let Mode::StartForm(form) = &mut self.mode {
                    form.offer(&interfaces);
                }
                self.interfaces = interfaces;
                vec![]
            }
            Incoming::Started { serial, tun } => {
                self.last_end.remove(&serial);
                self.info(format!("{serial}: start accepted (TUN {tun})"));
                vec![]
            }
            Incoming::Stopped { serial, outcome } => {
                self.info(format!("{serial}: {outcome}"));
                vec![]
            }
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

    pub(super) fn set_devices(&mut self, devices: Vec<DeviceInfo>) -> Vec<Command> {
        self.devices = devices;
        self.move_cursor(0);
        vec![]
    }
}
