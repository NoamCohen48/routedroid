//! `routedroid status`: the daemon's live device connections.

use anyhow::Result;
use routedroid_ipc::{Client, ConnectionInfo, Request, Response};

use super::answer;
use crate::output::{bytes, header, print_json, print_table};

pub async fn run(client: &Client, json: bool) -> Result<i32> {
    let connections = answer!(
        client.call_ok(Request::Status).await?,
        Response::Status { connections } => connections
    );
    if json {
        print_json(&connections)?;
    } else if connections.is_empty() {
        println!("no connections");
    } else {
        let names = [
            "SERIAL",
            "PHONE_IP",
            "LAN_IF",
            "TUN",
            "STATE",
            "TO_PHONE",
            "FROM_PHONE",
            "DROPPED",
        ];
        let mut rows = vec![header(&names)];
        rows.extend(connections.iter().map(row));
        print_table(&rows);
    }
    Ok(0)
}

fn row(connection: &ConnectionInfo) -> Vec<String> {
    let traffic = &connection.traffic;
    let phone_ip = connection.network.as_ref().map(|n| n.phone_ip.to_string());
    vec![
        connection.serial.clone(),
        phone_ip.unwrap_or_else(|| "-".into()),
        connection.lan_if.clone(),
        connection.tun.clone(),
        connection.state.to_string(),
        bytes(traffic.bytes_to_phone),
        bytes(traffic.bytes_from_phone),
        (traffic.dropped_malformed + traffic.dropped_congested).to_string(),
    ]
}
