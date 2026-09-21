//! `routedroid devices`

use crate::adb::{Adb, Device, DeviceState, DEFAULT_TIMEOUT};
use crate::device::Transport;
use crate::fault::Result;

pub async fn run(adb: &str, json: bool) -> Result<()> {
    let devices = Adb::new(adb, DEFAULT_TIMEOUT).devices().await?;
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
fn usable(d: &Device) -> Option<&'static str> {
    match &d.state {
        DeviceState::Unauthorized => Some("USB debugging not authorized on the phone"),
        DeviceState::Offline => Some("device is offline"),
        DeviceState::Other(_) => Some("device is not ready"),
        DeviceState::Device => Transport::classify(&d.serial).refusal(false),
    }
}
