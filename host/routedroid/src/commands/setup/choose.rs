//! Which interface phones join the LAN through, and how they get an
//! address there: from the flags, from the defaults, or by asking (`ask`).

use anyhow::Result;
use routedroid_helper_ipc::Interface;

use super::Usage;
use super::block::Block;

/// What the policy is to allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub lan_if: String,
    pub dhcp: bool,
    pub blocks: Vec<Block>,
}

impl Choice {
    /// "DHCP", "192.168.1.200/29", or "DHCP, 192.168.1.200/29".
    pub fn addresses(&self) -> String {
        let mut parts: Vec<String> = self.blocks.iter().map(ToString::to_string).collect();
        if self.dhcp {
            parts.insert(0, "DHCP".into());
        }
        parts.join(", ")
    }
}

/// Links that can carry phones once the policy allows them: up, with an
/// IPv4 address, and refused (if at all) only by the policy.
pub fn candidates(interfaces: &[Interface]) -> Vec<&Interface> {
    interfaces
        .iter()
        .filter(|link| link.up && !link.addresses.is_empty())
        .filter(|link| {
            link.ineligible.as_deref().is_none_or(|why| {
                why == "not in the helper policy" || why.starts_with("the helper policy")
            })
        })
        .collect()
}

/// The one to offer first, among those the policy allows already if any
/// (so running setup again keeps what it set up): where the default route
/// leaves, else the only one.
pub fn preferred<'a>(candidates: &[&'a Interface]) -> Option<&'a Interface> {
    let allowed: Vec<&Interface> = candidates
        .iter()
        .copied()
        .filter(|link| link.ineligible.is_none())
        .collect();
    let pool = if allowed.is_empty() {
        candidates
    } else {
        &allowed
    };
    pool.iter()
        .find(|link| link.default_route)
        .or(match pool {
            [only] => Some(only),
            _ => None,
        })
        .copied()
}

/// "lan0  192.168.1.10/24  default route  allowed: DHCP"
pub fn describe(link: &Interface) -> String {
    let addresses: Vec<String> = link
        .addresses
        .iter()
        .map(|net| format!("{}/{}", net.address, net.prefix))
        .collect();
    let mut line = format!("{:<12} {}", link.name, addresses.join(" "));
    if link.default_route {
        line += "  default route";
    }
    if link.ineligible.is_none() {
        let mut allowed: Vec<String> = link
            .phone_addresses
            .iter()
            .map(|net| format!("{}/{}", net.address, net.prefix))
            .collect();
        if link.dhcp {
            allowed.insert(0, "DHCP".into());
        }
        line += &format!("  allowed: {}", allowed.join(", "));
    }
    line
}

/// The choice the flags make: `lan_if` must be able to carry phones, and
/// every block must be on its LAN. No block means DHCP.
pub fn from_flags(
    interfaces: &[Interface],
    lan_if: &str,
    dhcp: bool,
    blocks: &[Block],
) -> Result<Choice> {
    let Some(link) = candidates(interfaces)
        .into_iter()
        .find(|l| l.name == lan_if)
    else {
        let why = match interfaces.iter().find(|link| link.name == lan_if) {
            Some(link) if link.addresses.is_empty() => "it has no IPv4 address".into(),
            Some(link) if !link.up => "it is down".into(),
            Some(link) => link.ineligible.clone().unwrap_or_default(),
            None => "there is no such interface".into(),
        };
        return Err(Usage(format!("phones cannot join through {lan_if}: {why}")).into());
    };
    if let Some(block) = blocks.iter().find(|block| !block.inside(link)) {
        return Err(Usage(format!(
            "{block} is not on {lan_if}'s LAN ({})",
            describe(link)
        ))
        .into());
    }
    Ok(Choice {
        lan_if: lan_if.into(),
        dhcp: dhcp || blocks.is_empty(),
        blocks: blocks.to_vec(),
    })
}

/// `--yes`: the preferred interface, by DHCP.
pub fn defaults(interfaces: &[Interface]) -> Result<Choice> {
    let candidates = candidates(interfaces);
    match preferred(&candidates) {
        Some(link) => from_flags(interfaces, &link.name, true, &[]),
        None if candidates.is_empty() => Err(none_usable(interfaces)),
        None => {
            let names: Vec<&str> = candidates.iter().map(|link| link.name.as_str()).collect();
            Err(Usage(format!(
                "more than one interface could carry phones ({}): choose with --lan-if",
                names.join(", ")
            ))
            .into())
        }
    }
}

/// No link can carry phones, with each one's reason.
pub fn none_usable(interfaces: &[Interface]) -> anyhow::Error {
    let reasons: Vec<String> = interfaces
        .iter()
        .filter_map(|link| {
            link.ineligible
                .as_ref()
                .map(|why| format!("  {}: {why}", link.name))
        })
        .collect();
    Usage(format!(
        "no interface can carry phones: one needs to be up, with an IPv4 address\n{}",
        reasons.join("\n")
    ))
    .into()
}

#[cfg(test)]
mod tests;
