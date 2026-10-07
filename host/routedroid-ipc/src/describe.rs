//! The words every client uses for the wire's states, so the CLI and the TUI
//! cannot describe the same connection two ways.

use std::fmt;

use crate::{ConnectionState, EndReason, Outcome, Screen};

impl fmt::Display for EndReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::StoppedEarly => "stopped before the connection was active",
            Self::Stopped => "stopped",
            Self::PhoneStopped => "stopped on the phone",
            Self::PhoneClosed => "the phone closed the connection",
        })
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clean { reason } => write!(f, "{reason}"),
            Self::Failed { kind, message } => write!(f, "failed ({kind}): {message}"),
        }
    }
}

/// The short word, for tables; `{:#}` is the sentence an attached `start`
/// prints, which tells the user what to do.
impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Starting => f.write_str("starting"),
            Self::InstallingApp if f.alternate() => {
                f.write_str("installing the Routedroid app on the phone")
            }
            Self::InstallingApp => f.write_str("installing app"),
            Self::WaitingForApp {
                screen: Some(screen),
            }
            | Self::Handshaking {
                screen: Some(screen),
            } if f.alternate() => f.write_str(match screen {
                Screen::Off => "the phone's screen is off: wake it and unlock it to continue",
                Screen::Locked => "the phone is locked: unlock it to continue",
            }),
            Self::WaitingForApp { .. } if f.alternate() => {
                f.write_str("waiting for the app to connect")
            }
            // The app asks for VPN permission once it has authenticated the host.
            Self::Handshaking { .. } if f.alternate() => f.write_str(
                "handshaking with the app (if the phone asks for VPN permission, answer it there)",
            ),
            Self::WaitingForApp { screen } => with_screen(f, "waiting for app", *screen),
            Self::Handshaking { screen } => with_screen(f, "handshaking", *screen),
            Self::Active => f.write_str("active"),
            Self::Reconnecting { wait_secs } if f.alternate() => write!(
                f,
                "the phone went away (unplugged?): its address is held for up to {wait_secs} s while it comes back"
            ),
            Self::Reconnecting { .. } => f.write_str("reconnecting"),
            Self::Stopping => f.write_str("stopping"),
            Self::Ended { outcome } => write!(f, "ended: {outcome}"),
        }
    }
}

fn with_screen(f: &mut fmt::Formatter<'_>, state: &str, screen: Option<Screen>) -> fmt::Result {
    match screen {
        None => f.write_str(state),
        Some(Screen::Off) => write!(f, "{state} (screen off)"),
        Some(Screen::Locked) => write!(f, "{state} (locked)"),
    }
}

/// "1.2 MB" for counters a person reads.
pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut value = count as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 => format!("{count} B"),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}
