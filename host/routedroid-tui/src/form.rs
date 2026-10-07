//! The "connect a phone" form: every start option, Tab between them, Enter
//! to submit. The LAN interface is picked from the daemon's list when it has
//! one. A draft is kept per phone, so Esc or a failed start loses nothing.

mod field;
mod input;
mod pick;
mod request;

use routedroid_ipc::InterfaceInfo;

pub use field::Field;
pub use input::LineInput;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartForm {
    pub serial: String,
    /// Interfaces a phone may join through; empty: type the name instead.
    pub choices: Vec<InterfaceInfo>,
    pub lan_if: LineInput,
    pub phone_ip: LineInput,
    pub dns: LineInput,
    pub mtu: LineInput,
    pub tun: LineInput,
    pub timeout: LineInput,
    pub reconnect_wait: LineInput,
    pub name: LineInput,
    /// Remember the phone with these options and connect it when plugged in.
    pub remember: bool,
    pub allow_network_adb: bool,
    pub focused: Field,
}

impl StartForm {
    pub fn new(serial: String, interfaces: &[InterfaceInfo]) -> Self {
        let mut form = Self {
            serial,
            choices: Vec::new(),
            lan_if: LineInput::default(),
            phone_ip: LineInput::default(),
            dns: LineInput::default(),
            mtu: LineInput::default(),
            tun: LineInput::default(),
            timeout: LineInput::default(),
            reconnect_wait: LineInput::default(),
            name: LineInput::default(),
            remember: false,
            allow_network_adb: false,
            focused: Field::LanIf,
        };
        form.offer(interfaces);
        form
    }

    /// The text field behind `field`, if it is one (the interface is one
    /// only when there is nothing to pick from).
    pub fn editable(&self, field: Field) -> Option<&LineInput> {
        Some(match field {
            Field::LanIf if self.choices.is_empty() => &self.lan_if,
            Field::LanIf | Field::Remember | Field::NetworkAdb => return None,
            Field::PhoneIp => &self.phone_ip,
            Field::Dns => &self.dns,
            Field::Mtu => &self.mtu,
            Field::Tun => &self.tun,
            Field::Timeout => &self.timeout,
            Field::ReconnectWait => &self.reconnect_wait,
            Field::Name => &self.name,
        })
    }

    pub fn input(&mut self, field: Field) -> Option<&mut LineInput> {
        Some(match field {
            Field::LanIf if self.choices.is_empty() => &mut self.lan_if,
            Field::LanIf | Field::Remember | Field::NetworkAdb => return None,
            Field::PhoneIp => &mut self.phone_ip,
            Field::Dns => &mut self.dns,
            Field::Mtu => &mut self.mtu,
            Field::Tun => &mut self.tun,
            Field::Timeout => &mut self.timeout,
            Field::ReconnectWait => &mut self.reconnect_wait,
            Field::Name => &mut self.name,
        })
    }

    /// The text shown for a field.
    pub fn shown(&self, field: Field) -> String {
        match field {
            Field::LanIf => self.lan_if.value().to_string(),
            Field::PhoneIp => self.phone_ip.value().to_string(),
            Field::Dns => self.dns.value().to_string(),
            Field::Mtu => self.mtu.value().to_string(),
            Field::Tun => self.tun.value().to_string(),
            Field::Timeout => self.timeout.value().to_string(),
            Field::ReconnectWait => self.reconnect_wait.value().to_string(),
            Field::Name => self.name.value().to_string(),
            Field::Remember => check_box(self.remember),
            Field::NetworkAdb => check_box(self.allow_network_adb),
        }
    }

    /// What is remembered of the phone: its name, and auto-connect.
    pub fn known(mut self, name: Option<&str>, auto: bool) -> Self {
        self.name = LineInput::new(name.unwrap_or_default());
        self.remember = auto;
        self
    }

    pub fn focus_next(&mut self) {
        self.focused = Field::ALL[(self.focused.index() + 1) % Field::ALL.len()];
    }

    pub fn focus_previous(&mut self) {
        let count = Field::ALL.len();
        self.focused = Field::ALL[(self.focused.index() + count - 1) % count];
    }
}

fn check_box(on: bool) -> String {
    if on { "[x]" } else { "[ ]" }.into()
}
