//! `adb reverse`: list, add, remove. Ownership decisions live in
//! `device::ports`; this file only speaks adb.

use super::AdbDevice;
use routedroid_ipc::fault::Result;

/// One line of `adb reverse --list`: `<serial-or-transport> tcp:9000 tcp:41234`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReverseMapping {
    /// The device-side endpoint, e.g. `tcp:9000`.
    pub remote: String,
    /// The host-side endpoint, e.g. `tcp:41234`.
    pub local: String,
}

impl ReverseMapping {
    /// Device port when `remote` is `tcp:<port>`.
    pub fn device_port(&self) -> Option<u16> {
        self.remote.strip_prefix("tcp:")?.parse().ok()
    }
}

pub fn parse_reverse_list(text: &str) -> Vec<ReverseMapping> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let _id = it.next()?;
            Some(ReverseMapping { remote: it.next()?.to_string(), local: it.next()?.to_string() })
        })
        .collect()
}

impl AdbDevice {
    pub async fn reverse_list(&self) -> Result<Vec<ReverseMapping>> {
        Ok(parse_reverse_list(&self.run(&["reverse", "--list"]).await?))
    }

    pub async fn reverse_add(&self, device_port: u16, host_port: u16) -> Result<()> {
        self.run(&["reverse", &format!("tcp:{device_port}"), &format!("tcp:{host_port}")]).await?;
        Ok(())
    }

    pub async fn reverse_remove(&self, device_port: u16) -> Result<()> {
        self.run(&["reverse", "--remove", &format!("tcp:{device_port}")]).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_list_lines() {
        let text = "UsbFfs tcp:9000 tcp:41234\n(reverse) tcp:9001 tcp:5\ngarbage\n";
        let l = parse_reverse_list(text);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0], ReverseMapping { remote: "tcp:9000".into(), local: "tcp:41234".into() });
        assert_eq!(l[0].device_port(), Some(9000));
        assert_eq!(ReverseMapping { remote: "localabstract:x".into(), local: "tcp:1".into() }.device_port(), None);
    }
}
