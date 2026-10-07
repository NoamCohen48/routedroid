//! What a start must pass before it is accepted: a phone adb can reach, and
//! one the helper's policy allows where it is asked to go. Either failing is
//! an error to the caller at once, not a connection that fails a moment
//! later.

use std::net::Ipv4Addr;

use routedroid_ipc::InterfaceInfo;

use super::{DeviceConnections, usage};
use crate::adb::DeviceState;
use crate::daemon::spec::StartSpec;
use crate::device::unusable;
use crate::fault::Result;

const POLICY: &str = "/etc/routedroid/helper.toml";

impl DeviceConnections {
    /// Refuse a phone adb cannot reach before a helper session is opened. The
    /// cached list is re-read first: a phone plugged in a moment ago is a
    /// likely thing to start on.
    pub(super) async fn check_attached(&self, serial: &str) -> Result<()> {
        let mut device = self.devices.get(serial);
        if device
            .as_ref()
            .is_none_or(|d| d.state != DeviceState::Device)
        {
            self.devices.refresh().await?;
            device = self.devices.get(serial);
        }
        match device {
            None => Err(usage(format!("{serial} is not attached"))),
            Some(device) if device.state == DeviceState::Device => Ok(()),
            Some(device) => {
                let reason = unusable(&device.state, serial).unwrap_or("device is not ready");
                Err(usage(format!("{serial}: {reason}")))
            }
        }
    }
}

/// The helper still has the last word: if it could not be asked
/// (`interfaces` is `None`), the start goes ahead and it decides.
pub(super) fn check_policy(interfaces: Option<&[InterfaceInfo]>, start: &StartSpec) -> Result<()> {
    let refused = interfaces.and_then(|i| refusal(i, start.lan_if.as_str(), start.phone_ip));
    refused.map_or(Ok(()), |refusal| Err(usage(refusal)))
}

/// Why the helper would refuse a phone on `lan_if` (as `phone_ip`, or by
/// DHCP when `None`), if it would.
fn refusal(
    interfaces: &[InterfaceInfo],
    lan_if: &str,
    phone_ip: Option<Ipv4Addr>,
) -> Option<String> {
    let Some(link) = interfaces.iter().find(|i| i.name == lan_if) else {
        return Some(format!("{lan_if}: no such interface"));
    };
    if let Some(why) = &link.ineligible {
        let hint = if why.contains("policy") {
            format!(" (allow it in {POLICY})")
        } else {
            String::new()
        };
        return Some(format!("{lan_if}: {why}{hint}"));
    }
    match phone_ip {
        Some(ip) if !link.phone_addresses.iter().any(|block| contains(block, ip)) => Some(format!(
            "{ip} is not a phone address the policy allows on {lan_if} ({})",
            allowed(link)
        )),
        None if !link.dhcp => Some(format!(
            "the policy does not allow DHCP on {lan_if}: give the phone an address with --phone-ip ({})",
            allowed(link)
        )),
        _ => None,
    }
}

fn allowed(link: &InterfaceInfo) -> String {
    let blocks: Vec<String> = link
        .phone_addresses
        .iter()
        .map(|b| format!("{}/{}", b.address, b.prefix))
        .collect();
    match (blocks.is_empty(), link.dhcp) {
        (true, _) => "it allows DHCP only".into(),
        (false, true) => format!("it allows {} or DHCP", blocks.join(", ")),
        (false, false) => format!("it allows {}", blocks.join(", ")),
    }
}

fn contains(block: &routedroid_ipc::Ipv4Net, ip: Ipv4Addr) -> bool {
    let mask = u32::MAX
        .checked_shl(32 - u32::from(block.prefix))
        .unwrap_or(0);
    u32::from(ip) & mask == u32::from(block.address) & mask
}

#[cfg(test)]
mod tests;
