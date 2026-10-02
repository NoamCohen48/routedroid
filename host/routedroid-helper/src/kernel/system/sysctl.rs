//! Per-interface IPv4 sysctls through `/proc/sys`. The interface name is a
//! literal directory: `eth0.100` is `conf/eth0.100/`, since only sysctl(8)'s
//! dotted key syntax swaps `.` and `/`.

use std::io;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::op::SysctlKey;

fn path(key: &SysctlKey) -> PathBuf {
    PathBuf::from(format!(
        "/proc/sys/net/ipv4/conf/{}/{}",
        key.ifname,
        key.leaf.as_str()
    ))
}

pub fn read(key: &SysctlKey) -> Result<Option<String>> {
    match std::fs::read_to_string(path(key)) {
        Ok(value) => Ok(Some(value.trim().to_owned())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {key}")),
    }
}

/// Only small non-negative integers: everything these leaves accept.
pub fn write(key: &SysctlKey, value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 3 || !value.bytes().all(|b| b.is_ascii_digit()) {
        bail!("refusing to write {value:?} to {key}");
    }
    std::fs::write(path(key), value).with_context(|| format!("write {key} = {value}"))
}

#[cfg(test)]
mod tests {
    use routedroid_helper_ipc::IfName;

    use super::*;
    use crate::op::Leaf;

    #[test]
    fn vlan_names_stay_literal() {
        let key = SysctlKey::new(IfName::new("eth0.100").unwrap(), Leaf::ProxyArp);
        assert_eq!(
            path(&key),
            PathBuf::from("/proc/sys/net/ipv4/conf/eth0.100/proxy_arp")
        );
    }

    #[test]
    fn values_are_validated_before_any_write() {
        let key = SysctlKey::new(IfName::new("nosuch0").unwrap(), Leaf::Forwarding);
        for bad in ["", "-1", "1\n", "1 ", "9999", "x"] {
            let err = write(&key, bad).unwrap_err();
            assert!(err.to_string().starts_with("refusing"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn a_missing_interface_reads_as_none() {
        let key = SysctlKey::new(IfName::new("nosuch0").unwrap(), Leaf::Forwarding);
        assert_eq!(read(&key).unwrap(), None);
    }
}
