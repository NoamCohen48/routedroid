//! The operator's decisions, which no controller can override: which LAN
//! interfaces may carry phones, which addresses a phone may request there,
//! and whether it may lease one from the LAN's DHCP server instead. Read
//! from a root-owned file on every `Start`; a missing, unsafe or
//! unparseable file refuses everything.
//!
//! ```toml
//! # /etc/routedroid/helper.toml
//! [[interface]]
//! name = "eno1"
//! phone_addresses = ["192.168.1.200/29"]  # requestable; bounds leases too
//! dhcp = true                             # default false
//! ```

use std::net::Ipv4Addr;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use routedroid_helper_ipc::IfName;
use serde::Deserialize;

mod cidr;

pub use cidr::{Cidr, mask};

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
    #[serde(default)]
    phone_addresses: Vec<Cidr>,
    #[serde(default)]
    dhcp: bool,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path).with_context(|| format!("policy {}", path.display()))?;
        ensure!(
            meta.is_file(),
            "policy {} is not a regular file",
            path.display()
        );
        let euid = rustix::process::geteuid().as_raw();
        ensure!(
            meta.uid() == euid || meta.uid() == 0,
            "policy {} is not owned by root",
            path.display()
        );
        ensure!(
            meta.mode() & 0o022 == 0,
            "policy {} is writable by others",
            path.display()
        );
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        text.parse()
            .with_context(|| format!("policy {}", path.display()))
    }

    /// The blocks phones may take on `lan_if`; empty when it is not allowed.
    pub fn phone_addresses(&self, lan_if: &str) -> &[Cidr] {
        self.interfaces
            .iter()
            .find(|i| i.name.as_str() == lan_if)
            .map_or(&[], |i| &i.phone_addresses)
    }

    /// Whether phones on `lan_if` may lease their address.
    pub fn dhcp(&self, lan_if: &str) -> bool {
        self.interfaces
            .iter()
            .any(|i| i.name.as_str() == lan_if && i.dhcp)
    }

    fn interface(&self, lan_if: &IfName) -> Result<&Interface> {
        match self.interfaces.iter().find(|i| &i.name == lan_if) {
            Some(interface) => Ok(interface),
            None => bail!("{lan_if} is not an interface the policy allows"),
        }
    }

    /// Why the operator does not let a phone request `phone_ip` on
    /// `lan_if`, if they don't.
    pub fn check(&self, lan_if: &IfName, phone_ip: Ipv4Addr) -> Result<()> {
        let interface = self.interface(lan_if)?;
        if !interface
            .phone_addresses
            .iter()
            .any(|b| b.contains(phone_ip))
        {
            bail!("{phone_ip} is not a phone address the policy allows on {lan_if}");
        }
        Ok(())
    }

    /// Why phones on `lan_if` may not lease an address, if they may not.
    pub fn check_dhcp(&self, lan_if: &IfName) -> Result<()> {
        ensure!(
            self.interface(lan_if)?.dhcp,
            "the policy does not allow DHCP on {lan_if}"
        );
        Ok(())
    }

    /// Why a leased `phone_ip` may not be used on `lan_if`: DHCP is not
    /// allowed there, or the lease falls outside the operator's blocks.
    pub fn check_leased(&self, lan_if: &IfName, phone_ip: Ipv4Addr) -> Result<()> {
        self.check_dhcp(lan_if)?;
        let blocks = &self.interface(lan_if)?.phone_addresses;
        if !blocks.is_empty() && !blocks.iter().any(|b| b.contains(phone_ip)) {
            bail!(
                "the leased {phone_ip} is outside the phone addresses the policy allows on {lan_if}"
            );
        }
        Ok(())
    }
}

impl std::str::FromStr for Policy {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let policy: Policy = toml::from_str(text)?;
        for (n, interface) in policy.interfaces.iter().enumerate() {
            if policy.interfaces[..n]
                .iter()
                .any(|other| other.name == interface.name)
            {
                bail!("interface {} is listed twice", interface.name);
            }
            if interface.phone_addresses.is_empty() && !interface.dhcp {
                bail!(
                    "interface {} allows neither phone_addresses nor dhcp",
                    interface.name
                );
            }
        }
        Ok(policy)
    }
}

#[cfg(test)]
mod tests;
