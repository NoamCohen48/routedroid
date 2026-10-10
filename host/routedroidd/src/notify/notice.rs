//! Which connection changes are worth a desktop notification, and in what
//! words: the phone on the LAN, gone, waiting to be unlocked, and ended
//! other than by the user's own stop. Each once, not on every repeat.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use routedroid_ipc::{ConnectionState, EndReason, Event, Outcome, Screen};

#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    pub serial: String,
    pub summary: String,
    pub body: String,
    pub urgent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moment {
    OnLan,
    Away,
    Unlock,
}

#[derive(Debug, Default)]
pub struct Notices {
    addresses: HashMap<String, Ipv4Addr>,
    last: HashMap<String, Moment>,
}

impl Notices {
    /// `phone`: what the user calls the phone with that serial.
    pub fn on(&mut self, event: &Event, phone: impl Fn(&str) -> String) -> Option<Notice> {
        let (serial, state) = match event {
            Event::Network { serial, network } => {
                self.addresses.insert(serial.clone(), network.phone_ip);
                return None;
            }
            Event::Connection { serial, state } => (serial, state),
            _ => return None,
        };
        let phone = phone(serial);
        let (moment, summary, body, urgent) = match state {
            ConnectionState::Active => {
                let back = self.last.get(serial) == Some(&Moment::Away);
                let summary = match back {
                    true => format!("{phone} is back on the LAN"),
                    false => format!("{phone} is on the LAN"),
                };
                let body = self
                    .addresses
                    .get(serial)
                    .map(|ip| format!("Its address is {ip}."));
                (Moment::OnLan, summary, body.unwrap_or_default(), false)
            }
            ConnectionState::Reconnecting { wait_secs } => (
                Moment::Away,
                format!("{phone} went away"),
                format!(
                    "Its address is held for up to {} while it comes back.",
                    duration(*wait_secs)
                ),
                false,
            ),
            ConnectionState::WaitingForApp {
                screen: Some(screen),
            }
            | ConnectionState::Handshaking {
                screen: Some(screen),
            } => {
                let body = match screen {
                    Screen::Off => "Its screen is off: wake it and unlock it to connect.",
                    Screen::Locked => "It is locked: unlock it to connect.",
                };
                (
                    Moment::Unlock,
                    format!("Unlock {phone}"),
                    body.into(),
                    false,
                )
            }
            ConnectionState::Ended { outcome } => return self.ended(serial, &phone, outcome),
            _ => return None,
        };
        if self.last.insert(serial.clone(), moment) == Some(moment) {
            return None;
        }
        Some(Notice {
            serial: serial.clone(),
            summary,
            body,
            urgent,
        })
    }

    fn ended(&mut self, serial: &str, phone: &str, outcome: &Outcome) -> Option<Notice> {
        self.addresses.remove(serial);
        self.last.remove(serial);
        let (body, urgent) = match outcome {
            Outcome::Failed { message, .. } => (message.clone(), true),
            Outcome::Clean {
                reason: EndReason::Stopped | EndReason::StoppedEarly,
            } => return None,
            Outcome::Clean { reason } => (sentence(&reason.to_string()), false),
        };
        Some(Notice {
            serial: serial.into(),
            summary: format!("{phone} is off the LAN"),
            body,
            urgent,
        })
    }
}

/// "stopped on the phone" → "Stopped on the phone."
fn sentence(words: &str) -> String {
    let mut chars = words.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
    format!("{}{}.", first.unwrap_or_default(), chars.as_str())
}

fn duration(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs} s"),
        _ if secs.is_multiple_of(60) => format!("{} min", secs / 60),
        _ => format!("{} min {} s", secs / 60, secs % 60),
    }
}

#[cfg(test)]
mod tests;
