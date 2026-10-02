//! UI state: what the panes draw. Pure `apply` methods fold daemon messages in;
//! they return follow-up commands instead of talking to the daemon themselves.

use std::collections::{BTreeMap, VecDeque};

use routedroid_ipc::{ConnectionInfo, DeviceInfo};

use crate::form::StartForm;
use crate::messages::{Command, Incoming};

mod events;

#[cfg(test)]
mod tests;

pub const LOG_CAPACITY: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    StartForm(StartForm),
    ConfirmStop { serial: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonLink {
    Connected,
    Disconnected { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: Level,
    pub text: String,
}

pub struct App {
    pub devices: Vec<DeviceInfo>,
    pub connections: BTreeMap<String, ConnectionInfo>,
    pub cursor: usize,
    pub log: VecDeque<LogLine>,
    pub daemon: DaemonLink,
    pub mode: Mode,
    pub quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            connections: BTreeMap::new(),
            cursor: 0,
            log: VecDeque::new(),
            daemon: DaemonLink::Connected,
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
        let last = self.devices.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push_log(Level::Info, text.into());
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push_log(Level::Error, text.into());
    }

    pub fn push_log(&mut self, level: Level, text: String) {
        if self.log.len() == LOG_CAPACITY {
            self.log.pop_front();
        }
        self.log.push_back(LogLine { level, text });
    }

    /// Folds one message in; returns what to ask the daemon next.
    pub fn apply(&mut self, incoming: Incoming) -> Vec<Command> {
        match incoming {
            Incoming::Connected => {
                self.daemon = DaemonLink::Connected;
                self.info("connected to routedroidd");
                vec![Command::RefreshDevices, Command::RefreshStatus]
            }
            Incoming::Disconnected { reason } => {
                self.error(format!("disconnected: {reason}"));
                self.daemon = DaemonLink::Disconnected { reason };
                vec![]
            }
            Incoming::Event(event) => self.apply_event(event),
            Incoming::Devices(devices) => {
                self.set_devices(devices);
                vec![]
            }
            Incoming::Connections(connections) => {
                self.connections = connections
                    .into_iter()
                    .map(|c| (c.serial.clone(), c))
                    .collect();
                vec![]
            }
            Incoming::Started { serial } => {
                self.info(format!("{serial}: start accepted"));
                vec![]
            }
            Incoming::Stopped { serial } => {
                self.info(format!("{serial}: stopped"));
                vec![]
            }
            Incoming::Failed { what, message } => {
                self.error(format!("{what} failed: {message}"));
                vec![]
            }
        }
    }

    pub(super) fn set_devices(&mut self, devices: Vec<DeviceInfo>) {
        self.devices = devices;
        self.move_cursor(0);
    }
}
