//! The operator's decisions, which no controller can override: which LAN
//! interfaces may carry phones, and which addresses a phone may be given
//! there. Read from a root-owned file on every `Start`; a missing, unsafe
//! or unparseable file refuses everything.
//!
//! ```toml
//! # /etc/routedroid/helper.toml
//! [[interface]]
//! name = "eno1"
//! phone_addresses = ["192.168.1.200/29"]
//! ```

use std::net::Ipv4Addr;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::{bail, ensure, Context, Result};
use routedroid_helper_ipc::IfName;
use serde::Deserialize;

mod cidr;

pub use cidr::{mask, Cidr};

pub const DEFAULT_PATH: &str = "/etc/routedroid/helper.toml";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default, rename = "interface")]
    interfaces: Vec<Interface>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Interface {
    name: IfName,
    phone_addresses: Vec<Cidr>,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path).with_context(|| format!("policy {}", path.display()))?;
        ensure!(meta.is_file(), "policy {} is not a regular file", path.display());
        let euid = rustix::process::geteuid().as_raw();
        ensure!(meta.uid() == euid || meta.uid() == 0, "policy {} is not owned by root", path.display());
        ensure!(meta.mode() & 0o022 == 0, "policy {} is writable by others", path.display());
        let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        text.parse().with_context(|| format!("policy {}", path.display()))
    }

    /// Why the operator does not allow `phone_ip` on `lan_if`, if they don't.
    pub fn check(&self, lan_if: &IfName, phone_ip: Ipv4Addr) -> Result<()> {
        let Some(interface) = self.interfaces.iter().find(|i| &i.name == lan_if) else {
            bail!("{lan_if} is not an interface the policy allows");
        };
        if !interface.phone_addresses.iter().any(|block| block.contains(phone_ip)) {
            bail!("{phone_ip} is not a phone address the policy allows on {lan_if}");
        }
        Ok(())
    }
}

impl std::str::FromStr for Policy {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let policy: Policy = toml::from_str(text)?;
        for (n, interface) in policy.interfaces.iter().enumerate() {
            if policy.interfaces[..n].iter().any(|other| other.name == interface.name) {
                bail!("interface {} is listed twice", interface.name);
            }
        }
        Ok(policy)
    }
}

#[cfg(test)]
mod tests;
