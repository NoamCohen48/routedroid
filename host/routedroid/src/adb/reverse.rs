//! `adb reverse` mapping: add, list, and remove only what is exactly ours.

use tracing::{info, warn};

use super::Adb;
use crate::fault::Result;

/// Parse `adb reverse --list` into (remote, local) pairs; lines look like
/// `<serial-or-transport> tcp:9000 tcp:41234`.
pub fn parse_reverse_list(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let _id = it.next()?;
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

/// Exactly one mapping for `device_port`, and it points at `host_port`.
pub fn list_has_exactly(text: &str, device_port: u16, host_port: u16) -> bool {
    let remote = format!("tcp:{device_port}");
    let ours: Vec<_> = parse_reverse_list(text).into_iter().filter(|(r, _)| *r == remote).collect();
    ours.len() == 1 && ours[0].1 == format!("tcp:{host_port}")
}

impl Adb {
    pub async fn reverse_list(&self) -> Result<String> {
        self.run(&["reverse", "--list"]).await
    }

    /// Device ports already mapped by anyone, so a new one can avoid them.
    pub async fn reverse_used_device_ports(&self) -> Result<Vec<u16>> {
        let list = self.reverse_list().await?;
        Ok(parse_reverse_list(&list)
            .into_iter()
            .filter_map(|(remote, _)| remote.strip_prefix("tcp:")?.parse().ok())
            .collect())
    }

    pub async fn reverse_add(&self, device_port: u16, host_port: u16) -> Result<()> {
        self.run(&["reverse", &format!("tcp:{device_port}"), &format!("tcp:{host_port}")]).await?;
        info!(device_port, host_port, "adb reverse mapping added");
        Ok(())
    }

    /// Remove the mapping only if the list still shows exactly ours.
    pub async fn reverse_remove_if_ours(&self, device_port: u16, host_port: u16) {
        let list = match self.reverse_list().await {
            Ok(l) => l,
            Err(e) => {
                warn!(error = %e, "could not list adb reverse mappings; leaving them untouched");
                return;
            }
        };
        if !list_has_exactly(&list, device_port, host_port) {
            warn!(device_port, host_port, list = %list.trim(), "reverse mapping is not ours any more; not removing");
            return;
        }
        match self.run(&["reverse", "--remove", &format!("tcp:{device_port}")]).await {
            Ok(_) => info!(device_port, "adb reverse mapping removed"),
            Err(e) => warn!(error = %e, "failed to remove adb reverse mapping"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches_exactly() {
        let text = "UsbFfs tcp:9000 tcp:41234\nUsbFfs tcp:9001 tcp:5\n";
        assert_eq!(parse_reverse_list(text).len(), 2);
        assert!(list_has_exactly(text, 9000, 41234));
        assert!(!list_has_exactly(text, 9000, 41235));
        assert!(!list_has_exactly(text, 9002, 41234));
        assert!(!list_has_exactly("", 9000, 41234));
        assert!(!list_has_exactly("X tcp:9000 tcp:41234\nX tcp:9000 tcp:1\n", 9000, 41234));
    }
}
