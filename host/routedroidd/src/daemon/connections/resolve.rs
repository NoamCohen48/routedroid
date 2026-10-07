//! Filling in what a `start` left out: which phone (a remembered name, or
//! the one attached that is not connected yet), its remembered options, and
//! which LAN (the remembered one, or the one the policy allows). What the
//! client did say always wins.

use routedroid_ipc::{InterfaceInfo, Phone, StartRequest};

use super::usage;
use crate::adb::{Device, DeviceState};
use crate::fault::Result;

/// What `resolve` looks at besides the request.
pub(super) struct Known<'a> {
    pub phones: &'a [Phone],
    pub attached: &'a [Device],
    /// Serials that already have a connection.
    pub connected: &'a [String],
    /// `None`: the helper could not be asked.
    pub interfaces: Option<&'a [InterfaceInfo]>,
}

pub(super) fn resolve(mut request: StartRequest, known: &Known<'_>) -> Result<StartRequest> {
    let serial = match request.serial.take() {
        Some(key) => known
            .phones
            .iter()
            .find(|p| p.name.as_deref() == Some(key.as_str()))
            .map_or(key, |p| p.serial.clone()),
        None => only_phone(known)?,
    };
    if let Some(phone) = known.phones.iter().find(|p| p.serial == serial) {
        // A remembered address belongs to the remembered LAN.
        let same_lan = request.lan_if.is_none() || request.lan_if == phone.lan_if;
        if request.phone_ip.is_none() && same_lan {
            request.phone_ip = phone.phone_ip;
        }
        request.lan_if = request.lan_if.or_else(|| phone.lan_if.clone());
        request.mtu = request.mtu.or(phone.mtu);
        request.dns = request.dns.or_else(|| phone.dns.clone());
        request.reconnect_secs = request.reconnect_secs.or(phone.reconnect_secs);
    }
    if request.lan_if.is_none() {
        request.lan_if = Some(only_lan(known.interfaces)?);
    }
    request.serial = Some(serial);
    Ok(request)
}

fn only_phone(known: &Known<'_>) -> Result<String> {
    let free: Vec<&str> = known
        .attached
        .iter()
        .filter(|d| d.state == DeviceState::Device && !known.connected.contains(&d.serial))
        .map(|d| d.serial.as_str())
        .collect();
    match free.as_slice() {
        [serial] => Ok((*serial).to_string()),
        [] if known.attached.is_empty() => Err(usage(
            "no phone attached: connect one by USB with USB debugging on".into(),
        )),
        [] => Err(usage(
            "no phone ready to connect (see `routedroid devices`)".into(),
        )),
        many => Err(usage(format!(
            "{} phones attached ({}): say which one",
            many.len(),
            many.join(", ")
        ))),
    }
}

fn only_lan(interfaces: Option<&[InterfaceInfo]>) -> Result<String> {
    let Some(interfaces) = interfaces else {
        return Err(usage(
            "could not ask the helper which interface phones may join: name one with --lan-if"
                .into(),
        ));
    };
    let allowed: Vec<&str> = interfaces
        .iter()
        .filter(|i| i.ineligible.is_none())
        .map(|i| i.name.as_str())
        .collect();
    match allowed.as_slice() {
        [name] => Ok((*name).to_string()),
        [] => Err(usage(
            "no interface may carry phones yet: run `sudo routedroid setup`".into(),
        )),
        many => Err(usage(format!(
            "phones may join through {}: say which with --lan-if",
            many.join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests;
