//! The "connect a phone" form: every start option, Tab between them, Enter
//! to submit. The LAN interface is picked from the daemon's list when it has
//! one. A draft is kept per phone, so Esc or a failed start loses nothing.

mod input;
mod request;

use routedroid_ipc::InterfaceInfo;

pub use input::LineInput;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    LanIf,
    PhoneIp,
    Dns,
    Mtu,
    Tun,
    Timeout,
    NetworkAdb,
}

impl Field {
    pub const ALL: [Field; 7] = [
        Field::LanIf,
        Field::PhoneIp,
        Field::Dns,
        Field::Mtu,
        Field::Tun,
        Field::Timeout,
        Field::NetworkAdb,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::LanIf => "LAN interface",
            Field::PhoneIp => "Phone IP (empty: DHCP)",
            Field::Dns => "DNS (empty: automatic, \"none\", or a list)",
            Field::Mtu => "MTU (empty: default)",
            Field::Tun => "TUN name (empty: next phoneN)",
            Field::Timeout => "App connect timeout (e.g. 90s, 2m)",
            Field::NetworkAdb => "Allow network ADB (Space toggles)",
        }
    }

    fn index(self) -> usize {
        Field::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0)
    }
}

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
            allow_network_adb: false,
            focused: Field::LanIf,
        };
        form.offer(interfaces);
        form
    }

    /// New interface list: keep the pick if it is still there, else take the
    /// one with the default route.
    pub fn offer(&mut self, interfaces: &[InterfaceInfo]) {
        self.choices = interfaces
            .iter()
            .filter(|i| i.ineligible.is_none())
            .cloned()
            .collect();
        let kept = self.choices.iter().any(|i| i.name == self.lan_if.value());
        if !kept {
            let best = self
                .choices
                .iter()
                .find(|i| i.default_route)
                .or(self.choices.first());
            if let Some(best) = best {
                self.lan_if = LineInput::new(&best.name);
            }
        }
    }

    /// The blocks the helper allows phones to take on the picked interface.
    pub fn phone_hint(&self) -> Option<String> {
        let picked = self
            .choices
            .iter()
            .find(|i| i.name == self.lan_if.value())?;
        let blocks: Vec<String> = picked
            .phone_addresses
            .iter()
            .map(|net| format!("{}/{}", net.address, net.prefix))
            .collect();
        (!blocks.is_empty()).then(|| format!("allowed: {}", blocks.join(" ")))
    }

    pub fn picking(&self) -> bool {
        self.focused == Field::LanIf && !self.choices.is_empty()
    }

    /// Cycle the picked interface by `step`.
    pub fn pick(&mut self, step: isize) {
        let count = self.choices.len() as isize;
        if count == 0 {
            return;
        }
        let at = self
            .choices
            .iter()
            .position(|i| i.name == self.lan_if.value());
        let next = at.map_or(0, |at| (at as isize + step).rem_euclid(count)) as usize;
        self.lan_if = LineInput::new(&self.choices[next].name);
    }

    /// The text field behind `field`, if it is one (the interface is one
    /// only when there is nothing to pick from).
    pub fn editable(&self, field: Field) -> Option<&LineInput> {
        Some(match field {
            Field::LanIf if self.choices.is_empty() => &self.lan_if,
            Field::LanIf | Field::NetworkAdb => return None,
            Field::PhoneIp => &self.phone_ip,
            Field::Dns => &self.dns,
            Field::Mtu => &self.mtu,
            Field::Tun => &self.tun,
            Field::Timeout => &self.timeout,
        })
    }

    pub fn input(&mut self, field: Field) -> Option<&mut LineInput> {
        Some(match field {
            Field::LanIf if self.choices.is_empty() => &mut self.lan_if,
            Field::LanIf | Field::NetworkAdb => return None,
            Field::PhoneIp => &mut self.phone_ip,
            Field::Dns => &mut self.dns,
            Field::Mtu => &mut self.mtu,
            Field::Tun => &mut self.tun,
            Field::Timeout => &mut self.timeout,
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
            Field::NetworkAdb => if self.allow_network_adb { "[x]" } else { "[ ]" }.into(),
        }
    }

    pub fn focus_next(&mut self) {
        self.focused = Field::ALL[(self.focused.index() + 1) % Field::ALL.len()];
    }

    pub fn focus_previous(&mut self) {
        let count = Field::ALL.len();
        self.focused = Field::ALL[(self.focused.index() + count - 1) % count];
    }
}
