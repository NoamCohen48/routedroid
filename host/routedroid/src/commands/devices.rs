//! `routedroid devices`: what adb sees and whether each phone can be connected.

use anyhow::{bail, Result};
use routedroid_ipc::{Client, DeviceInfo, Request, Response};

use crate::output::{print_json, print_table, state_word};

pub async fn run(client: &mut Client, json: bool) -> Result<i32> {
    let devices = match client.call_ok(Request::Devices).await? {
        Response::Devices { devices } => devices,
        other => bail!("unexpected answer to devices: {other:?}"),
    };
    if json {
        print_json(&devices)?;
    } else if devices.is_empty() {
        println!("no devices attached");
    } else {
        print_table(&devices.iter().map(row).collect::<Vec<_>>());
    }
    Ok(0)
}

fn row(device: &DeviceInfo) -> Vec<String> {
    let model = device.model.clone().unwrap_or_else(|| "-".into());
    let verdict = match (&device.connection, &device.unusable_reason) {
        (Some(state), _) => format!("connected: {}", state_word(state)),
        (None, Some(reason)) => format!("not usable: {reason}"),
        (None, None) => "usable".into(),
    };
    vec![device.serial.clone(), model, verdict]
}
