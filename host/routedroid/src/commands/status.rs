//! `routedroid status`: the daemon's live device connections.

use anyhow::{bail, Result};
use routedroid_ipc::{Client, ConnectionInfo, Request, Response};

use crate::output::{print_json, print_table, state_word};

pub async fn run(client: &mut Client, json: bool) -> Result<i32> {
    let connections = match client.call_ok(Request::Status).await? {
        Response::Status { connections } => connections,
        other => bail!("unexpected answer to status: {other:?}"),
    };
    if json {
        print_json(&connections)?;
    } else if connections.is_empty() {
        println!("no connections");
    } else {
        let mut rows = vec![header()];
        rows.extend(connections.iter().map(row));
        print_table(&rows);
    }
    Ok(0)
}

fn header() -> Vec<String> {
    ["SERIAL", "PHONE_IP", "LAN_IF", "TUN", "STATE", "TO_PHONE", "FROM_PHONE"].map(String::from).to_vec()
}

fn row(connection: &ConnectionInfo) -> Vec<String> {
    vec![
        connection.serial.clone(),
        connection.phone_ip.to_string(),
        connection.lan_if.clone(),
        connection.tun.clone(),
        state_word(&connection.state),
        connection.packets_to_phone.to_string(),
        connection.packets_from_phone.to_string(),
    ]
}
