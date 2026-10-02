//! `routedroid interfaces`: where a phone may join the LAN, and why not elsewhere.

use anyhow::Result;
use routedroid_ipc::{Client, InterfaceInfo, Ipv4Net, Request, Response};

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
    let verdict = match &interface.ineligible {
        None => nets(&interface.phone_addresses),
        Some(reason) => format!("no: {reason}"),
    };
    vec![
        interface.name.clone(),
        state.into(),
        nets(&interface.addresses),
        verdict,
    ]
}

fn nets(nets: &[Ipv4Net]) -> String {
    if nets.is_empty() {
        return "-".into();
    }
    let nets: Vec<String> = nets
        .iter()
        .map(|net| format!("{}/{}", net.address, net.prefix))
        .collect();
    nets.join(" ")
}
