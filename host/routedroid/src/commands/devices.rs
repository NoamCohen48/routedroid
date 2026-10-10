//! `routedroid devices`: what adb sees and whether each phone can be connected.

use anyhow::Result;
use routedroid_ipc::{Client, DeviceInfo, Request, Response};

use super::answer;
use crate::output::{header, print_json, print_table};

pub async fn run(client: &Client, json: bool) -> Result<i32> {
    let devices =
        answer!(client.call_ok(Request::Devices).await?, Response::Devices { devices } => devices);
    if json {
        print_json(&devices)?;
    } else if devices.is_empty() {
        println!("no devices attached");
    } else {
        let mut rows = vec![header(&["SERIAL", "NAME", "MODEL", "STATUS"])];
        rows.extend(devices.iter().map(row));
        print_table(&rows);
    }
    Ok(0)
}

fn row(device: &DeviceInfo) -> Vec<String> {
    let model = device.model.clone().unwrap_or_else(|| "-".into());
    let verdict = match (&device.connection, &device.unusable_reason) {
        (Some(state), _) => format!("connected: {state}"),
        (None, Some(reason)) => format!("not usable: {reason}"),
        (None, None) => "usable".into(),
    };
    let name = device.name.clone().unwrap_or_else(|| "-".into());
    vec![device.serial.clone(), name, model, verdict]
}
