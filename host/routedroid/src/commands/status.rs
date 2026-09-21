//! `routedroid status`: the daemon's live sessions.

use anyhow::{bail, Result};
use routedroid_ipc::{Client, Request, Response, SessionInfo};

use crate::output::{print_json, print_table, state_word};

pub async fn run(client: &mut Client, json: bool) -> Result<i32> {
    let sessions = match client.call_ok(Request::Status).await? {
        Response::Status { sessions } => sessions,
        other => bail!("unexpected answer to status: {other:?}"),
    };
    if json {
        print_json(&sessions)?;
    } else if sessions.is_empty() {
        println!("no sessions");
    } else {
        let mut rows = vec![header()];
        rows.extend(sessions.iter().map(row));
        print_table(&rows);
    }
    Ok(0)
}

fn header() -> Vec<String> {
    ["SERIAL", "PHONE_IP", "LAN_IF", "TUN", "STATE", "TO_PHONE", "FROM_PHONE"].map(String::from).to_vec()
}

fn row(session: &SessionInfo) -> Vec<String> {
    vec![
        session.serial.clone(),
        session.phone_ip.to_string(),
        session.lan_if.clone(),
        session.tun.clone(),
        state_word(&session.state),
        session.packets_to_phone.to_string(),
        session.packets_from_phone.to_string(),
    ]
}
