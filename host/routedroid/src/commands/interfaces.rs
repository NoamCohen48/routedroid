//! `routedroid interfaces`: where a phone may join the LAN, and why not elsewhere.

use anyhow::Result;
use routedroid_ipc::{Client, InterfaceInfo, Request, Response};

use super::answer;
use crate::output::{header, print_json, print_table};

pub async fn run(client: &Client, json: bool) -> Result<i32> {
    let interfaces = answer!(
        client.call_ok(Request::Interfaces).await?,
        Response::Interfaces { interfaces } => interfaces
    );
    if json {
        print_json(&interfaces)?;
    } else if interfaces.is_empty() {
        println!("no network interfaces");
    } else {
        let mut rows = vec![header(&["INTERFACE", "STATE", "IPV4", "PHONES"])];
        rows.extend(interfaces.iter().map(row));
        print_table(&rows);
    }
    Ok(0)
}

fn row(interface: &InterfaceInfo) -> Vec<String> {
    let state = match (interface.up, interface.default_route) {
        (true, true) => "up, default route",
        (true, false) => "up",
        (false, _) => "down",
    };
    let addresses: Vec<String> = interface
        .addresses
        .iter()
        .map(|net| format!("{}/{}", net.address, net.prefix))
        .collect();
    let verdict = match &interface.ineligible {
        None => "allowed".to_string(),
        Some(reason) => format!("no: {reason}"),
    };
    let addresses = if addresses.is_empty() {
        "-".into()
    } else {
        addresses.join(" ")
    };
    vec![interface.name.clone(), state.into(), addresses, verdict]
}
