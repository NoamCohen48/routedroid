//! Transport policy (decision 0001, deferred gate 5). Version 1 has only
//! been verified over USB: a network ADB transport would itself be routed
//! through the VPN once the default route is installed, and whether Android
//! keeps the debugging socket out of the tunnel is untested. The refusal
//! lives here, in one place, and `start` applies it; nothing else in the
//! host cares how the phone is attached.

use crate::fault::{Fault, Kind, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Usb,
    /// `emulator-N`: a TCP transport, but one that never leaves the host, so
    /// the VPN default route cannot cut it. Accepted.
    Emulator,
    Network,
    Invalid,
}

impl Transport {
    pub fn classify(serial: &str) -> Self {
        if serial.is_empty()
            || serial.len() > 128
            || serial.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Self::Invalid;
        }
        if serial
            .strip_prefix("emulator-")
            .is_some_and(|n| n.parse::<u16>().is_ok())
        {
            return Self::Emulator;
        }
        // `host:port`, `[v6]:port`, and mDNS names like `adb-XXXX._adb-tls-connect._tcp`.
        if serial.contains(':') || serial.contains("._tcp") {
            return Self::Network;
        }
        Self::Usb
    }

    /// Why a device with this serial cannot be started, if it cannot.
    pub fn refusal(self, allow_network: bool) -> Option<&'static str> {
        match self {
            Self::Usb | Self::Emulator => None,
            Self::Network if allow_network => None,
            Self::Network => {
                Some("network ADB is not verified in version 1 (use USB, or --allow-network-adb)")
            }
            Self::Invalid => Some("not a valid ADB serial"),
        }
    }

    /// Apply the policy to the serial `start` was given.
    pub fn check(serial: &str, allow_network: bool) -> Result<()> {
        let t = Self::classify(serial);
        match t.refusal(allow_network) {
            None => {
                if t == Self::Network {
                    tracing::warn!(
                        serial,
                        "network ADB allowed by flag; the VPN route may cut this connection"
                    );
                }
                Ok(())
            }
            Some(why) if t == Self::Invalid => {
                Err(Fault::msg(Kind::Usage, format!("serial {serial:?}: {why}")))
            }
            Some(why) => Err(Fault::msg(
                Kind::Transport,
                format!("serial {serial:?}: {why}"),
            )),
        }
    }
}
