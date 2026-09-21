//! `routedroid devices`

use crate::adb::{Adb, DeviceState, Transport, DEFAULT_TIMEOUT};
use crate::fault::Result;

pub async fn run(adb: &str, json: bool) -> Result<()> {
    let devices = Adb::devices(adb, DEFAULT_TIMEOUT).await?;
    if json {
        let rows: Vec<serde_json::Value> = devices
            .iter()
            .map(|d| {
                serde_json::json!({
                    "serial": d.serial,
                    "state": format!("{:?}", d.state).to_lowercase(),
                    "model": d.model,
                    "usable": usable(d).is_none(),
                    "reason": usable(d),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        return Ok(());
    }
    if devices.is_empty() {
        println!("no devices attached");
        return Ok(());
    }
    for d in &devices {
        let model = d.model.as_deref().unwrap_or("-");
        match usable(d) {
            None => println!("{:<24} {:<20} usable", d.serial, model),
            Some(why) => println!("{:<24} {:<20} not usable: {why}", d.serial, model),
        }
    }
    Ok(())
}

/// None when Routedroid can start on this device; otherwise the reason.
fn usable(d: &crate::adb::Device) -> Option<&'static str> {
    match (&d.state, d.transport) {
        (DeviceState::Unauthorized, _) => Some("USB debugging not authorized on the phone"),
        (DeviceState::Offline, _) => Some("device is offline"),
        (DeviceState::Other(_), _) => Some("device is not ready"),
        (DeviceState::Device, Transport::Network) => Some("network ADB is not supported in version 1 (use USB)"),
        (DeviceState::Device, Transport::Invalid) => Some("unrecognised serial"),
        (DeviceState::Device, Transport::Usb | Transport::Emulator) => None,
    }
}
