//! Serial classification (§1): USB serials are accepted; anything that names
//! a network endpoint is refused in version 1.

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
        if serial.is_empty() || serial.len() > 128 || serial.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Self::Invalid;
        }
        if serial.strip_prefix("emulator-").is_some_and(|n| n.parse::<u16>().is_ok()) {
            return Self::Emulator;
        }
        // `host:port`, `[v6]:port`, and mDNS names like `adb-XXXX._adb-tls-connect._tcp`.
        if serial.contains(':') || serial.contains("._tcp") {
            return Self::Network;
        }
        Self::Usb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(Transport::classify("R58M12345AB"), Transport::Usb);
        assert_eq!(Transport::classify("0123456789ABCDEF"), Transport::Usb);
        assert_eq!(Transport::classify("192.168.1.5:5555"), Transport::Network);
        assert_eq!(Transport::classify("adb-R58M12345AB-abcdef._adb-tls-connect._tcp"), Transport::Network);
        assert_eq!(Transport::classify("emulator-5554"), Transport::Emulator);
        assert_eq!(Transport::classify("emulator-x"), Transport::Usb);
        assert_eq!(Transport::classify(""), Transport::Invalid);
        assert_eq!(Transport::classify("has space"), Transport::Invalid);
    }
}
