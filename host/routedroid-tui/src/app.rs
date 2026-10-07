//! UI state: what the panes draw. Pure `apply` methods fold daemon messages in;
//! they return follow-up commands instead of talking to the daemon themselves.

use std::collections::{BTreeMap, HashMap};

use routedroid_ipc::{ConnectionInfo, DeviceInfo, InterfaceInfo};

use crate::form::StartForm;
use crate::messages::Command;

mod apply;
mod events;
mod log;
mod rates;
mod rows;

pub use log::{Level, Log};
pub use rates::Rates;

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
    /// What adb lists, as the daemon last said.
    attached: Vec<DeviceInfo>,
    /// The table's rows: `attached`, then live connections to phones adb
    /// no longer lists (`rows.rs`).
    pub devices: Vec<DeviceInfo>,
    pub connections: BTreeMap<String, ConnectionInfo>,
    pub interfaces: Vec<InterfaceInfo>,
    pub last_end: HashMap<String, LastEnd>,
    /// Each live connection's throughput over the last minute.
    pub rates: HashMap<String, Rates>,
    /// The form as each phone's user last left it.
    drafts: HashMap<String, StartForm>,
    pub cursor: usize,
    pub log: Log,
    pub daemon: DaemonLink,
    pub mode: Mode,
    pub quit: bool,
    /// The device list last logged, so a repeat is not logged again.
    devices_line: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            attached: Vec::new(),
            devices: Vec::new(),
            connections: BTreeMap::new(),
            interfaces: Vec::new(),
            last_end: HashMap::new(),
            rates: HashMap::new(),
            drafts: HashMap::new(),
            cursor: 0,
            log: Log::default(),
            daemon: DaemonLink::Connecting,
            mode: Mode::Normal,
            quit: false,
            devices_line: None,
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
            None => {
                let known = self.devices.iter().find(|d| d.serial == serial);
                let (name, auto) = known.map_or((None, false), |d| (d.name.clone(), d.auto));
                StartForm::new(serial, &self.interfaces).known(name.as_deref(), auto)
            }
        };
        self.mode = Mode::StartForm(Box::new(form));
        vec![Command::RefreshInterfaces]
    }

    /// Leave the form, keeping what was typed for next time.
    pub fn close_form(&mut self, form: Box<StartForm>) {
        self.drafts.insert(form.serial.clone(), *form);
        self.mode = Mode::Normal;
    }
}
