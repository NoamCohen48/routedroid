//! What a remembered phone may hold.

use routedroid_helper_ipc::{IfName, MTU_RANGE};
use routedroid_ipc::Phone;

use super::usage;
use crate::fault::Result;

const MAX_NAME: usize = 32;

/// A name is a word a person types: unique, and nobody else's serial.
pub(super) fn check(phone: &Phone, list: &[Phone]) -> Result<()> {
    if phone.serial.trim().is_empty() {
        return Err(usage("a remembered phone needs its serial".into()));
    }
    let others = list.iter().filter(|p| p.serial != phone.serial);
    if let Some(name) = &phone.name {
        let word = |c: char| c.is_alphanumeric() || "-_.".contains(c);
        if name.is_empty() || name.len() > MAX_NAME || !name.chars().all(word) {
            return Err(usage(format!(
                "name {name:?}: up to {MAX_NAME} letters, digits, '-', '_' or '.'"
            )));
        }
        for other in others {
            if other.name.as_deref() == Some(name) || &other.serial == name {
                return Err(usage(format!("{name} already names {}", other.serial)));
            }
        }
    }
    if let Some(lan_if) = &phone.lan_if {
        IfName::new(lan_if).map_err(|e| usage(format!("lan_if: {e}")))?;
    }
    if let Some(mtu) = phone.mtu
        && !MTU_RANGE.contains(&mtu)
    {
        return Err(usage(format!("mtu {mtu} is outside {MTU_RANGE:?}")));
    }
    Ok(())
}
