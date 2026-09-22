//! The "connect a phone" form: three text fields, Tab between them, Enter to submit.

use std::net::Ipv4Addr;

use anyhow::{Context, Result};
use routedroid_ipc::StartRequest;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    LanIf,
    PhoneIp,
    Dns,
}

impl Field {
    pub const ALL: [Field; 3] = [Field::LanIf, Field::PhoneIp, Field::Dns];

    pub fn label(self) -> &'static str {
        match self {
            Field::LanIf => "LAN interface",
            Field::PhoneIp => "Phone IP",
            Field::Dns => "DNS (optional, comma-separated)",
        }
    }

    fn index(self) -> usize {
        Field::ALL.iter().position(|field| *field == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartForm {
    pub serial: String,
    pub lan_if: String,
    pub phone_ip: String,
    pub dns: String,
    pub focused: Field,
}

impl StartForm {
    pub fn new(serial: String) -> Self {
        Self { serial, lan_if: String::new(), phone_ip: String::new(), dns: String::new(), focused: Field::LanIf }
    }

    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::LanIf => &self.lan_if,
            Field::PhoneIp => &self.phone_ip,
            Field::Dns => &self.dns,
        }
    }

    fn value_mut(&mut self) -> &mut String {
        match self.focused {
            Field::LanIf => &mut self.lan_if,
            Field::PhoneIp => &mut self.phone_ip,
            Field::Dns => &mut self.dns,
        }
    }

    pub fn focus_next(&mut self) {
        self.focused = Field::ALL[(self.focused.index() + 1) % Field::ALL.len()];
    }

    pub fn focus_previous(&mut self) {
        self.focused = Field::ALL[(self.focused.index() + Field::ALL.len() - 1) % Field::ALL.len()];
    }

    pub fn insert(&mut self, character: char) {
        if !character.is_control() {
            self.value_mut().push(character);
        }
    }

    pub fn backspace(&mut self) {
        self.value_mut().pop();
    }

    /// Validates the fields; the error names the offending one.
    pub fn to_request(&self) -> Result<StartRequest> {
        let lan_if = self.lan_if.trim();
        anyhow::ensure!(!lan_if.is_empty(), "LAN interface is required");
        let phone_ip: Ipv4Addr = self
            .phone_ip
            .trim()
            .parse()
            .with_context(|| format!("phone IP {:?} is not an IPv4 address", self.phone_ip))?;
        let dns = self
            .dns
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.parse::<Ipv4Addr>().with_context(|| format!("DNS {entry:?} is not an IPv4 address")))
            .collect::<Result<Vec<_>>>()?;
        Ok(StartRequest {
            serial: self.serial.clone(),
            lan_if: lan_if.to_string(),
            phone_ip,
            tun: None,
            mtu: None,
            dns,
            connect_timeout_secs: None,
            allow_network_adb: false,
        })
    }
}
